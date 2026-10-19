//! Process ownership for integration tests only. No test assertions or protocol
//! implementation lives in the Lua server fixture.

use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use herdr_nvim::protocol::{Context, OwnerControl, Reply, Request, VERSION};
use serde::Deserialize;

pub(crate) fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

pub(crate) fn command(root: &Path) -> Command {
    let mut command = Command::new("nvim");
    command
        .args(["--headless", "-u", "NONE", "-i", "NONE"])
        .current_dir(root)
        .env(
            "HERDR_NVIM_CONFIG",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../home/nvim/.config/nvim"),
        )
        .env("HERDR_NVIM_TESTS", tests_dir())
        .env("HERDR_NVIM_SCRATCH", root)
        .env("NVIM_LOG_FILE", root.join("nvim.log"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"));
    command
}

pub(crate) struct Scratch(pub(crate) PathBuf);
impl Scratch {
    pub(crate) fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Keep AF_UNIX paths short, private, and independent of the live
        // editor socket directory (including on macOS with a long TMPDIR).
        let path = PathBuf::from(format!("/tmp/dnt-{}-{nonce}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Deserialize)]
pub(crate) struct Ready {
    pub(crate) socket: PathBuf,
    pub(crate) context: Context,
    pub(crate) pid: u32,
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) struct Fixture {
    child: Process,
    pub(crate) scratch: Scratch,
    pub(crate) ready: Ready,
}

impl Fixture {
    pub(crate) fn start() -> Self {
        Self::start_reserved(Scratch::new(), None)
    }

    pub(crate) fn start_reserved(scratch: Scratch, registration: Option<&str>) -> Self {
        let ready_file = scratch.0.join("ready.json");
        let log = fs::File::create(scratch.0.join("editor.log")).unwrap();
        let mut command = command(&scratch.0);
        command.env_remove("HERDR_NVIM_RESERVED_REGISTRATION");
        if let Some(registration) = registration {
            command.env("HERDR_NVIM_RESERVED_REGISTRATION", registration);
        }
        command
            .arg("-l")
            .arg(tests_dir().join("fixtures/editor.lua"))
            .env("HERDR_NVIM_READY", &ready_file)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log);
        // Own the child immediately, including failure to start the Lua fixture.
        let mut child = Process(command.spawn().unwrap());
        let started = Instant::now();
        while !ready_file.exists()
            && started.elapsed() < Duration::from_secs(5)
            && child.0.try_wait().unwrap().is_none()
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            ready_file.exists(),
            "fixture did not register: {}",
            fs::read_to_string(scratch.0.join("editor.log")).unwrap()
        );
        let ready = serde_json::from_slice(&fs::read(ready_file).unwrap()).unwrap();
        Self {
            child,
            scratch,
            ready,
        }
    }

    pub(crate) fn command(&self, expression: &str, argument: &str) -> Command {
        let expression = format!(
            "luaeval({}, {})",
            serde_json::to_string(expression).unwrap(),
            serde_json::to_string(argument).unwrap()
        );
        let mut command = Command::new("nvim");
        command
            .arg("--server")
            .arg(&self.ready.socket)
            .args(["--remote-expr", &expression]);
        command
    }

    pub(crate) fn signal(&self, signal: nix::sys::signal::Signal) {
        let pid = nix::unistd::Pid::from_raw(i32::try_from(self.child.0.id()).unwrap());
        nix::sys::signal::kill(pid, signal).unwrap();
    }

    pub(crate) fn lua(&self, expression: &str, argument: &str) -> String {
        let output = herdrkit::process::output(
            self.command(expression, argument),
            Duration::from_secs(3),
            64 * 1024,
        )
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    pub(crate) fn raw(&self, request: &str) -> Reply {
        serde_json::from_str(&self.lua("require('dots.herdr').request(_A)", request)).unwrap()
    }

    pub(crate) fn owner(&self, control: OwnerControl) -> Reply {
        self.request(&Request {
            protocol: VERSION,
            context: self.ready.context.clone(),
            operation: herdr_nvim::protocol::Operation::Owner { control },
        })
    }

    pub(crate) fn request(&self, request: &Request) -> Reply {
        self.raw(&serde_json::to_string(request).unwrap())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.ready.socket.as_os_str().is_empty() {
            let mut command = Command::new("nvim");
            command
                .arg("--server")
                .arg(&self.ready.socket)
                .args(["--remote-expr", "luaeval('fixture.stop()')"]);
            let _ = herdrkit::process::output(command, Duration::from_secs(1), 4096);
        }
        let start = Instant::now();
        while self.child.0.try_wait().ok().flatten().is_none()
            && start.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.0.kill();
        let _ = self.child.0.wait();
        if !self.ready.socket.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.ready.socket);
        }
    }
}
