//! Pane-less, plugin-owned services. Hooks ensure readiness; a service owns its
//! timer and work. Recovery after a crash occurs on the next hook, not by Herdr
//! supervision. No caller PID is ever used as authority to kill a process.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{Client, Invocation};

const CHILD_ENV: &str = "HERDRKIT_SERVICE_CHILD";
const FRAME_LIMIT: u64 = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);
const START_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("service filesystem or control I/O failed")]
    Io(#[from] std::io::Error),
    #[error("service control message is invalid")]
    Json(#[from] serde_json::Error),
    #[error("service owner lookup failed")]
    Herdr(#[from] crate::Error),
    #[error("{0}")]
    State(String),
}
pub type Result<T> = std::result::Result<T, Error>;

/// Plugin behavior must enqueue slow work, not block the control/health loop.
pub trait Handler {
    fn request(&mut self, payload: Value) -> std::result::Result<Value, String>;
    fn tick(&mut self) -> std::result::Result<(), String>;
    fn status(&self) -> Value;
}

/// Paths and identity for one service belonging to one concrete Herdr server.
#[derive(Debug, Clone)]
pub struct Service {
    client: Client,
    plugin_id: String,
    plugin_root: PathBuf,
    identity: String,
    directory: PathBuf,
    socket: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    identity: String,
    command: Control,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
enum Control {
    Status,
    Wake(Value),
    Shutdown,
}

#[derive(Debug, Serialize, Deserialize)]
struct Reply {
    identity: String,
    result: std::result::Result<Value, String>,
}

impl Service {
    pub fn new(invocation: &Invocation, name: &str) -> Result<Self> {
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(Error::State(
                "service name must be ASCII letters, digits or hyphens".into(),
            ));
        }
        let identity = serde_json::to_string(&(
            invocation.client().server_id()?,
            invocation.plugin_id(),
            name,
            invocation.state_dir(),
            invocation.plugin_root(),
        ))?;
        let key = format!("{:x}", Sha256::digest(identity.as_bytes()));
        let directory = invocation.state_dir().join("services").join(&key);
        // Darwin's Unix socket path limit is too short for plugin state paths.
        // A private hash-named directory keeps the endpoint short and isolated.
        let socket_dir = std::env::temp_dir().join(format!("hks-{}", &key[..24]));
        private_directory(&directory)?;
        private_directory(&socket_dir)?;
        Ok(Self {
            client: invocation.client().clone(),
            plugin_id: invocation.plugin_id().into(),
            plugin_root: invocation.plugin_root().into(),
            identity,
            directory,
            socket: socket_dir.join("control.sock"),
        })
    }

    pub fn is_worker(&self) -> bool {
        std::env::var(CHILD_ENV).is_ok_and(|value| value == self.identity)
    }

    pub fn state_dir(&self) -> &std::path::Path {
        &self.directory
    }

    pub fn status(&self) -> Result<Value> {
        self.call(Control::Status)
    }

    pub fn wake(&self, payload: Value) -> Result<Value> {
        self.ensure()?;
        self.call(Control::Wake(payload))
    }

    pub fn shutdown(&self) -> Result<()> {
        self.call(Control::Shutdown).map(|_| ())
    }

    /// Start at most one child, with a bounded readiness check and launch cooldown.
    pub fn ensure(&self) -> Result<()> {
        if self.status().is_ok() {
            return Ok(());
        }
        let deadline = Instant::now() + START_TIMEOUT;
        let launch = loop {
            if let Some(lock) = lock(&self.directory.join("launch.lock"))? {
                break lock;
            }
            if self.status().is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::State("service launch is busy".into()));
            }
            thread::sleep(Duration::from_millis(50));
        };
        if self.status().is_ok() {
            return Ok(());
        }
        // An unresponsive lock owner is not authority to spawn another worker.
        let Some(worker_lock) = lock(&self.directory.join("worker.lock"))? else {
            return Err(Error::State(
                "service is running but its control endpoint is unresponsive".into(),
            ));
        };
        drop(worker_lock);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::State(e.to_string()))?
            .as_secs();
        let attempt = self.directory.join("launch-attempt");
        match fs::read_to_string(&attempt) {
            Ok(value) => {
                let previous: u64 = value
                    .parse()
                    .map_err(|e| Error::State(format!("invalid launch timestamp: {e}")))?;
                if now >= previous && now - previous < 30 {
                    return Err(Error::State(
                        "service restart cooling down; inspect service.log".into(),
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let temporary = attempt.with_extension("tmp");
        fs::write(&temporary, now.to_string())?;
        fs::rename(&temporary, &attempt)?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(self.directory.join("service.log"))?;
        let mut child = Command::new(std::env::current_exe()?)
            .env(CHILD_ENV, &self.identity)
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?;
        while Instant::now() < deadline {
            if self.status().is_ok() {
                thread::spawn(move || {
                    let _ = child.wait();
                });
                drop(launch);
                return Ok(());
            }
            if let Some(status) = child.try_wait()? {
                return Err(Error::State(format!(
                    "service exited before readiness: {status}"
                )));
            }
            thread::sleep(Duration::from_millis(50));
        }
        child.kill()?;
        child.wait()?;
        Err(Error::State(
            "service did not become ready within eight seconds".into(),
        ))
    }

    fn call(&self, command: Control) -> Result<Value> {
        let mut stream = UnixStream::connect(&self.socket)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        write_frame(
            &mut stream,
            &Request {
                identity: self.identity.clone(),
                command,
            },
        )?;
        let reply: Reply = read_frame(&stream)?;
        if reply.identity != self.identity {
            return Err(Error::State("service identity mismatch".into()));
        }
        reply.result.map_err(Error::State)
    }

    /// Run in the detached child. Timer work and control handlers must be bounded.
    /// Owner checks happen every ten seconds; thirty seconds of owner errors stop
    /// the worker. A replaced socket or disabled/unlinked plugin stops it directly.
    pub fn run(&self, mut handler: impl Handler) -> Result<()> {
        if !self.is_worker() {
            return Err(Error::State(
                "service loop requires an owned worker launch".into(),
            ));
        }
        nix::unistd::setsid().map_err(|e| Error::State(format!("detaching service: {e}")))?;
        let Some(_lock) = lock(&self.directory.join("worker.lock"))? else {
            return Ok(());
        };
        match fs::remove_file(&self.socket) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let listener = UnixListener::bind(&self.socket)?;
        fs::set_permissions(&self.socket, fs::Permissions::from_mode(0o600))?;
        listener.set_nonblocking(true)?;
        let mut owner_checked: Option<Instant> = None;
        let mut owner_error = None;
        let mut ticked: Option<Instant> = None;
        loop {
            if owner_checked.is_none_or(|at| at.elapsed() >= Duration::from_secs(10)) {
                owner_checked = Some(Instant::now());
                match self.owner_alive() {
                    Ok(true) => owner_error = None,
                    Ok(false) => break,
                    Err(error) => {
                        eprintln!("service owner check: {error}");
                        let since = owner_error.get_or_insert_with(Instant::now);
                        if since.elapsed() >= Duration::from_secs(30) {
                            break;
                        }
                    }
                }
            }
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    stream.set_read_timeout(Some(IO_TIMEOUT))?;
                    stream.set_write_timeout(Some(IO_TIMEOUT))?;
                    match read_frame::<Request>(&stream) {
                        Ok(request) if request.identity == self.identity => {
                            let shutdown = matches!(request.command, Control::Shutdown);
                            let result = match request.command {
                                Control::Status => Ok(handler.status()),
                                Control::Wake(value) => handler.request(value),
                                Control::Shutdown => Ok(Value::Null),
                            };
                            if let Err(e) = write_frame(
                                &mut stream,
                                &Reply {
                                    identity: self.identity.clone(),
                                    result,
                                },
                            ) {
                                eprintln!("service response: {e}");
                            }
                            if shutdown {
                                break;
                            }
                        }
                        Ok(_) => eprintln!("rejected foreign service identity"),
                        Err(e) => eprintln!("service request: {e}"),
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            if owner_error.is_none()
                && ticked.is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
            {
                ticked = Some(Instant::now());
                if let Err(e) = handler.tick() {
                    eprintln!("service tick: {e}");
                }
            }
            thread::sleep(Duration::from_millis(25));
        }
        fs::remove_file(&self.socket)?;
        Ok(())
    }

    fn owner_alive(&self) -> Result<bool> {
        let identity: (String, String, String, PathBuf, PathBuf) =
            serde_json::from_str(&self.identity)?;
        if self.client.server_id()? != identity.0 {
            return Ok(false);
        }
        Ok(self
            .client
            .plugin_registration(&self.plugin_id)?
            .is_some_and(|p| p.enabled && p.plugin_root == self.plugin_root))
    }
}

fn private_directory(path: &std::path::Path) -> Result<()> {
    fs::create_dir_all(path)?;
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(Error::State(
            "service directory must not be a symlink".into(),
        ));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn lock(path: &std::path::Path) -> Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

fn read_frame<T: serde::de::DeserializeOwned>(stream: &UnixStream) -> Result<T> {
    let mut frame = Vec::new();
    BufReader::new(stream)
        .take(FRAME_LIMIT + 1)
        .read_until(b'\n', &mut frame)?;
    if frame.len() as u64 > FRAME_LIMIT || frame.last() != Some(&b'\n') {
        return Err(Error::State(
            "service frame is oversized or incomplete".into(),
        ));
    }
    Ok(serde_json::from_slice(&frame)?)
}

fn write_frame(stream: &mut UnixStream, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    if bytes.len() as u64 > FRAME_LIMIT {
        return Err(Error::State("service frame exceeds limit".into()));
    }
    stream.write_all(&bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Server, Step};

    #[test]
    fn ownership_requires_same_server_enabled_plugin_and_root() {
        let response = |enabled, root: &str| {
            Step::reply(format!(
                r#"{{"id":"{{id}}","result":{{"type":"plugin_list","plugins":[{{"plugin_id":"fixture","plugin_root":"{root}","enabled":{enabled}}}]}}}}"#
            ))
        };
        let server = Server::start(vec![
            response(true, "/fixture"),
            response(false, "/fixture"),
            response(true, "/replacement"),
        ]);
        let identity = serde_json::to_string(&(
            server.client().server_id().unwrap(),
            "fixture",
            "status",
            PathBuf::from("/state"),
            PathBuf::from("/fixture"),
        ))
        .unwrap();
        let mut service = Service {
            client: server.client(),
            plugin_id: "fixture".into(),
            plugin_root: "/fixture".into(),
            identity,
            directory: "/unused".into(),
            socket: "/unused".into(),
        };
        assert!(service.owner_alive().unwrap());
        assert!(!service.owner_alive().unwrap());
        assert!(!service.owner_alive().unwrap());
        let replacement = Server::start(Vec::new());
        service.client = replacement.client();
        assert!(!service.owner_alive().unwrap());
        assert!(
            replacement.requests().is_empty(),
            "replaced endpoints must not be queried as the old owner"
        );
    }

    #[test]
    fn control_frames_reject_truncation_and_oversize() {
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        sender.write_all(b"{\"identity\":\"x\"").unwrap();
        sender.shutdown(std::net::Shutdown::Write).unwrap();
        assert!(matches!(
            read_frame::<Request>(&receiver),
            Err(Error::State(_))
        ));
        let (mut sender, receiver) = UnixStream::pair().unwrap();
        let writer = thread::spawn(move || {
            let _ = sender.write_all(&vec![b'x'; 65537]);
        });
        assert!(matches!(
            read_frame::<Request>(&receiver),
            Err(Error::State(_))
        ));
        drop(receiver);
        writer.join().unwrap();
    }

    #[test]
    fn worker_lock_is_exclusive_and_released_on_drop() {
        let directory =
            std::env::temp_dir().join(format!("herdrkit-lock-test-{}", std::process::id()));
        private_directory(&directory).unwrap();
        let path = directory.join("worker.lock");
        let first = lock(&path).unwrap().unwrap();
        assert!(lock(&path).unwrap().is_none());
        drop(first);
        assert!(lock(&path).unwrap().is_some());
        fs::remove_dir_all(directory).unwrap();
    }
}
