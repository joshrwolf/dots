//! Resolve an inherited or explicitly named local Herdr session.
//!
//! Inside Herdr, its injected socket is authoritative. Outside Herdr the caller
//! must select a session; discovery never guesses a default/focused session and
//! never starts or attaches to a server. Session paths come from Herdr's own
//! `session list --json` output rather than a duplicated directory convention.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncRead, AsyncReadExt as _};

use crate::{Client, Error, Result, env};

const DEADLINE: Duration = Duration::from_secs(3);
const MAX_LISTING: usize = 1024 * 1024;
const MAX_DIAGNOSTIC: usize = 64 * 1024;

/// A validated selection; environment capture is separate from resolution.
#[derive(Debug, Clone)]
pub struct Request {
    target: Target,
    timeout: Option<String>,
    executable: PathBuf,
}

#[derive(Debug, Clone)]
enum Target {
    Inherited(PathBuf),
    Session(String),
}

impl Request {
    /// Explicit names override inherited sockets. Empty/relative sockets and
    /// empty/control-bearing names are rejected before any subprocess starts.
    pub fn new(
        session: Option<String>,
        inherited_socket: Option<PathBuf>,
        worktree_timeout: Option<&str>,
    ) -> Result<Self> {
        let target = if let Some(name) = session {
            if name.trim().is_empty() || name.chars().any(char::is_control) {
                return Err(Error::InvalidConnectionTarget {
                    reason: "session name is empty or contains controls",
                });
            }
            Target::Session(name)
        } else if let Some(socket) = inherited_socket {
            validate_socket(&socket)?;
            Target::Inherited(socket)
        } else {
            return Err(Error::MissingConnectionTarget);
        };
        if let Some(timeout) = worktree_timeout {
            crate::socket::parse_worktree_timeout(timeout)?;
        }
        Ok(Self {
            target,
            timeout: worktree_timeout.map(str::to_owned),
            executable: PathBuf::from("herdr"),
        })
    }

    /// Pin CLI discovery to an absolute executable for GUI-launched callers.
    pub fn with_executable(mut self, executable: PathBuf) -> Result<Self> {
        if !executable.is_absolute() || executable.as_os_str().as_bytes().contains(&0) {
            return Err(Error::InvalidConnectionTarget {
                reason: "Herdr executable must be an absolute path without NUL bytes",
            });
        }
        self.executable = executable;
        Ok(self)
    }

    pub fn from_env(session: Option<&str>) -> Result<Self> {
        let timeout = env::optional_string("HERDR_WORKTREE_TIMEOUT_SECS")?;
        Self::new(
            session.map(str::to_owned),
            env::optional_path("HERDR_SOCKET_PATH"),
            timeout.as_deref(),
        )
    }

    /// Resolve without opening a workspace, attaching a UI, or starting a server.
    pub async fn resolve(self) -> Result<Connection> {
        match self.target {
            Target::Inherited(socket) => connection(socket, None, self.timeout.as_deref()),
            Target::Session(name) => {
                let listing = discover(&mut discovery_command(&self.executable)).await?;
                let socket = select(&listing, &name)?;
                connection(socket, Some(name), self.timeout.as_deref())
            }
        }
    }
}

/// The chosen endpoint and the ordinary typed Herdr client bound to it.
#[derive(Debug, Clone)]
pub struct Connection {
    client: Client,
    session: Option<String>,
}

impl Connection {
    pub fn client(&self) -> &Client {
        &self.client
    }
    pub fn endpoint(&self) -> &Path {
        self.client.endpoint()
    }
    /// Present for an explicit selection; inherited sockets do not imply a name.
    pub fn session_name(&self) -> Option<&str> {
        self.session.as_deref()
    }
    pub fn into_client(self) -> Client {
        self.client
    }
}

/// Capture the current environment, then resolve an explicit or inherited target.
pub async fn resolve(session: Option<&str>) -> Result<Connection> {
    Request::from_env(session)?.resolve().await
}

fn connection(
    socket: PathBuf,
    session: Option<String>,
    timeout: Option<&str>,
) -> Result<Connection> {
    Ok(Connection {
        client: Client::configured(socket, timeout)?,
        session,
    })
}

fn validate_socket(socket: &Path) -> Result<()> {
    if !socket.is_absolute() || socket.as_os_str().as_bytes().contains(&0) {
        return Err(Error::InvalidConnectionTarget {
            reason: "socket path must be absolute and contain no NUL bytes",
        });
    }
    Ok(())
}

#[derive(Deserialize)]
struct Listing {
    sessions: Vec<Session>,
}

#[derive(Deserialize)]
struct Session {
    name: String,
    running: bool,
    socket_path: PathBuf,
}

fn select(bytes: &[u8], name: &str) -> Result<PathBuf> {
    let listing: Listing = serde_json::from_slice(bytes).map_err(Error::SessionDiscoveryDecode)?;
    let mut matches = listing
        .sessions
        .into_iter()
        .filter(|session| session.name == name);
    let session = matches.next().ok_or_else(|| Error::SessionNotFound {
        name: name.to_owned(),
    })?;
    if matches.next().is_some() {
        return Err(Error::AmbiguousSession {
            name: name.to_owned(),
        });
    }
    if !session.running {
        return Err(Error::SessionStopped {
            name: name.to_owned(),
        });
    }
    validate_socket(&session.socket_path)?;
    Ok(session.socket_path)
}

fn discovery_command(executable: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(executable);
    command.args(["session", "list", "--json"]);
    // Listing must not inherit an unrelated socket/session selection. Herdr's
    // normal configuration environment remains intact for its session registry.
    command.env_remove("HERDR_SOCKET_PATH");
    command
}

async fn bounded(mut reader: impl AsyncRead + Unpin, maximum: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = vec![0_u8; 8192];
    loop {
        let count = reader
            .read(&mut chunk)
            .await
            .map_err(Error::SessionDiscoveryIo)?;
        if count == 0 {
            return Ok(bytes);
        }
        if bytes.len().saturating_add(count) > maximum {
            return Err(Error::SessionDiscoveryTooLarge);
        }
        bytes.extend_from_slice(chunk.get(..count).ok_or(Error::SessionDiscoveryTooLarge)?);
    }
}

async fn discover(command: &mut tokio::process::Command) -> Result<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(Error::SessionDiscoveryIo)?;
    let stdout = child.stdout.take().ok_or(Error::InvalidConnectionTarget {
        reason: "session discovery stdout unavailable",
    })?;
    let stderr = child.stderr.take().ok_or(Error::InvalidConnectionTarget {
        reason: "session discovery stderr unavailable",
    })?;
    let result = tokio::time::timeout(DEADLINE, async {
        let (status, stdout, _) = tokio::try_join!(
            async { child.wait().await.map_err(Error::SessionDiscoveryIo) },
            bounded(stdout, MAX_LISTING),
            bounded(stderr, MAX_DIAGNOSTIC),
        )?;
        if !status.success() {
            return Err(Error::SessionDiscoveryExit {
                code: status.code(),
            });
        }
        Ok(stdout)
    })
    .await
    .unwrap_or(Err(Error::SessionDiscoveryTimeout));
    if result.is_err() {
        // A failed read/limit/timeout must not leave a child or full pipe behind.
        // kill waits/reaps as well; wait also covers an already-exited process.
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &[u8] = br#"{"sessions":[
      {"name":"default","default":true,"running":true,"socket_path":"/tmp/herdr/default/api.sock","session_dir":"/tmp/herdr/default"},
      {"name":"project","default":false,"running":true,"socket_path":"/tmp/herdr/project/api.sock","session_dir":"/tmp/herdr/project"},
      {"name":"stopped","default":false,"running":false,"socket_path":"/tmp/herdr/stopped/api.sock","session_dir":"/tmp/herdr/stopped"}
    ]}"#;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn external_launch_requires_selection_even_when_default_exists() {
        assert!(matches!(
            Request::new(None, None, None),
            Err(Error::MissingConnectionTarget)
        ));
        assert_eq!(
            select(LISTING, "default").unwrap(),
            Path::new("/tmp/herdr/default/api.sock")
        );
        assert_eq!(
            select(LISTING, "project").unwrap(),
            Path::new("/tmp/herdr/project/api.sock")
        );
    }

    #[test]
    fn inherited_connection_needs_no_plugin_context_or_session_discovery() {
        let request = Request::new(
            None,
            Some("/tmp/herdr/inherited/api.sock".into()),
            Some("45"),
        )
        .unwrap();
        let connection = runtime().block_on(request.resolve()).unwrap();
        assert_eq!(
            connection.endpoint(),
            Path::new("/tmp/herdr/inherited/api.sock")
        );
        assert_eq!(connection.session_name(), None);
    }

    #[test]
    fn explicit_selection_overrides_inherited_socket_without_guessing() {
        let request =
            Request::new(Some("project".into()), Some("/tmp/other.sock".into()), None).unwrap();
        assert!(matches!(request.target, Target::Session(name) if name == "project"));
    }

    #[test]
    fn stopped_missing_ambiguous_and_malformed_sessions_fail_before_connection() {
        assert!(matches!(
            select(LISTING, "missing"),
            Err(Error::SessionNotFound { .. })
        ));
        assert!(matches!(
            select(LISTING, "stopped"),
            Err(Error::SessionStopped { .. })
        ));
        assert!(matches!(select(br#"{"sessions":[{"name":"x","running":true,"socket_path":"/tmp/a"},{"name":"x","running":true,"socket_path":"/tmp/b"}]}"#, "x"), Err(Error::AmbiguousSession { .. })));
        assert!(matches!(
            select(
                br#"{"sessions":[{"name":"x","running":true,"socket_path":"relative"}]}"#,
                "x"
            ),
            Err(Error::InvalidConnectionTarget { .. })
        ));
        assert!(matches!(
            select(
                br#"{"sessions":[{"name":"x","socket_path":"/tmp/a"}]}"#,
                "x"
            ),
            Err(Error::SessionDiscoveryDecode(_))
        ));
    }

    #[test]
    fn invalid_inputs_are_rejected_at_construction() {
        for name in ["", " ", "name\nother"] {
            assert!(Request::new(Some(name.into()), None, None).is_err());
        }
        assert!(Request::new(None, Some("relative.sock".into()), None).is_err());
        assert!(matches!(
            Request::new(None, Some("/tmp/a.sock".into()), Some("0")),
            Err(Error::InvalidWorktreeTimeout { .. })
        ));
    }

    #[test]
    fn bounded_reader_accepts_exact_limit_and_rejects_overflow() {
        runtime().block_on(async {
            assert_eq!(bounded(&b"abc"[..], 3).await.unwrap(), b"abc");
            assert!(matches!(
                bounded(&b"abcd"[..], 3).await,
                Err(Error::SessionDiscoveryTooLarge)
            ));
        });
    }

    #[test]
    fn discovery_checks_exit_status_and_drains_both_streams() {
        runtime().block_on(async {
            let mut command = tokio::process::Command::new("/bin/sh");
            command.args([
                "-c",
                "printf '%s' '{\"sessions\":[]}'; printf '%s' 'neutral diagnostic' >&2; exit 7",
            ]);
            assert!(matches!(
                discover(&mut command).await,
                Err(Error::SessionDiscoveryExit { code: Some(7) })
            ));
        });
    }

    /// Process lifecycle: the verify tier selects `acceptance` modules.
    mod acceptance {
        use super::*;

        #[test]
        fn discovery_deadline_kills_and_reaps_the_process() {
            runtime().block_on(async {
                let mut command = tokio::process::Command::new("/bin/sleep");
                command.arg("30");
                let start = std::time::Instant::now();
                assert!(matches!(
                    discover(&mut command).await,
                    Err(Error::SessionDiscoveryTimeout)
                ));
                assert!(start.elapsed() < Duration::from_secs(6));
            });
        }
    }

    #[test]
    fn discovery_only_requests_listing_and_removes_inherited_endpoint() {
        let command = discovery_command(Path::new("/opt/bin/herdr"));
        let command = command.as_std();
        assert_eq!(command.get_program(), "/opt/bin/herdr");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["session", "list", "--json"]
        );
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == "HERDR_SOCKET_PATH" && value.is_none())
        );
    }

    #[test]
    fn inherited_timeout_uses_the_same_policy_as_plugin_clients() {
        #[derive(serde::Serialize)]
        struct WorktreeOperation {}
        impl crate::socket::Request for WorktreeOperation {
            const METHOD: &'static str = "worktree.list";
        }
        let request = Request::new(None, Some("/tmp/herdr/api.sock".into()), Some("45")).unwrap();
        let connection = runtime().block_on(request.resolve()).unwrap();
        assert_eq!(
            connection.client().request_timeout(&WorktreeOperation {}),
            Some(Duration::from_secs(45))
        );
    }
}
