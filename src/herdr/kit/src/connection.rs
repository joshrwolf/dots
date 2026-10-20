//! Resolve an inherited or explicitly named local Herdr session.
//!
//! Inside Herdr, its injected socket is authoritative. Outside Herdr the caller
//! must select a session; discovery never guesses a default/focused session and
//! never starts or attaches to a server. Session paths come from Herdr's own
//! `session list --json` output rather than a duplicated directory convention.

use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use serde::Deserialize;

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
                let listing = discover(discovery_command(&self.executable)).await?;
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

fn discovery_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.args(["session", "list", "--json"]);
    // Listing must not inherit an unrelated socket/session selection. Herdr's
    // normal configuration environment remains intact for its session registry.
    command.env_remove("HERDR_SOCKET_PATH");
    command
}

/// Bounded by the shared runner: a deadline, an output limit, or an I/O failure
/// kills the process group and reaps the child.
async fn discover(command: Command) -> Result<Vec<u8>> {
    listing(
        dots_process::Runner::new(DEADLINE, MAX_DIAGNOSTIC)
            .stdout_limit(MAX_LISTING)
            .run(command)
            .await,
    )
}

fn listing(output: std::result::Result<Output, dots_process::Error>) -> Result<Vec<u8>> {
    use dots_process::Error as Process;
    let output = output.map_err(|error| match error {
        Process::Timeout(_) => Error::SessionDiscoveryTimeout,
        Process::OutputLimit { .. } => Error::SessionDiscoveryTooLarge,
        Process::Runtime(source)
        | Process::Spawn(source)
        | Process::Read { source, .. }
        | Process::Sink(source)
        | Process::Wait(source) => Error::SessionDiscoveryIo(source),
        error => Error::SessionDiscoveryIo(std::io::Error::other(error)),
    })?;
    if !output.status.success() {
        return Err(Error::SessionDiscoveryExit {
            code: output.status.code(),
        });
    }
    Ok(output.stdout)
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
    fn discovery_requires_success_and_maps_runner_failures() {
        use std::os::unix::process::ExitStatusExt as _;
        use std::process::ExitStatus;
        let exited = |code: i32| Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: br#"{"sessions":[]}"#.to_vec(),
            stderr: b"neutral diagnostic".to_vec(),
        };
        assert_eq!(listing(Ok(exited(0))).unwrap(), br#"{"sessions":[]}"#);
        assert!(matches!(
            listing(Ok(exited(7))),
            Err(Error::SessionDiscoveryExit { code: Some(7) })
        ));
        assert!(matches!(
            listing(Err(dots_process::Error::Timeout(DEADLINE))),
            Err(Error::SessionDiscoveryTimeout)
        ));
        assert!(matches!(
            listing(Err(dots_process::Error::OutputLimit {
                stream: "stdout",
                limit: MAX_LISTING
            })),
            Err(Error::SessionDiscoveryTooLarge)
        ));
        assert!(matches!(
            listing(Err(dots_process::Error::Spawn(std::io::Error::other(
                "missing"
            )))),
            Err(Error::SessionDiscoveryIo(_))
        ));
    }

    /// Host acceptance: the verify tier selects `acceptance` modules.
    mod acceptance {
        use super::*;

        /// Stock Herdr on `PATH` lists an isolated home's registry without
        /// starting a server; its sole session is reported as stopped.
        #[test]
        fn stock_herdr_listing_is_decoded_and_stopped_sessions_fail() {
            let herdr = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|directory| directory.join("herdr"))
                .find(|path| path.is_file())
                .expect("stock Herdr must be on PATH");
            let root =
                std::env::temp_dir().join(format!("herdrkit-discovery-{}", std::process::id()));
            std::fs::create_dir_all(&root).unwrap();
            let mut command = discovery_command(&herdr);
            for (name, _) in std::env::vars_os() {
                let text = name.to_string_lossy();
                if text.starts_with("HERDR_") || text.starts_with("XDG_") {
                    command.env_remove(&name);
                }
            }
            command
                .current_dir(&root)
                .env("HOME", &root)
                .env("XDG_CONFIG_HOME", root.join("config"))
                .env("XDG_STATE_HOME", root.join("state"));
            let listing = runtime().block_on(discover(command));
            std::fs::remove_dir_all(&root).unwrap();
            let listing = listing.unwrap();
            assert!(matches!(
                select(&listing, "default"),
                Err(Error::SessionStopped { .. })
            ));
            assert!(matches!(
                select(&listing, "missing"),
                Err(Error::SessionNotFound { .. })
            ));
        }
    }

    #[test]
    fn discovery_only_requests_listing_and_removes_inherited_endpoint() {
        let command = discovery_command(Path::new("/opt/bin/herdr"));
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
