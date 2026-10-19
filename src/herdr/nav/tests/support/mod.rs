//! Hermetic stock Herdr process ownership. No live user session is contacted.
use std::fs::{self, File};
use std::os::unix::fs::DirBuilderExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, ensure};
use herdrkit::{Client, PaneId, TerminalId};
use serde::Deserialize;
use serde_json::{Value, json};

const CONFIG: &str = r#"
onboarding = false
[terminal]
default_shell = "/bin/sh"
shell_mode = "non_login"
[session]
resume_agents_on_restore = false
[keys]
prefix = "ctrl+s"
new_tab = "prefix+t"
previous_tab = "prefix+h"
[[keys.command]]
key = "prefix+shift+o"
type = "plugin_action"
command = "herdr-nav.previous-workspace"
[[keys.command]]
key = "prefix+o"
type = "plugin_action"
command = "herdr-nav.back"
[[keys.command]]
key = "prefix+shift+i"
type = "plugin_action"
command = "herdr-nav.forward"
"#;

#[derive(Debug)]
pub(crate) struct Process(pub(crate) Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub(crate) struct History {
    pub(crate) entries: Vec<TerminalId>,
    pub(crate) current: Option<TerminalId>,
}
#[derive(Debug, Deserialize)]
struct Navigation {
    history: History,
}
#[derive(Debug, Deserialize)]
struct Status {
    navigation: Navigation,
    last_error: Option<String>,
}

pub(crate) fn eventually<T>(
    timeout: Duration,
    mut check: impl FnMut() -> Result<Option<T>>,
) -> Result<T> {
    let start = Instant::now();
    loop {
        if let Some(value) = check()? {
            return Ok(value);
        }
        ensure!(
            start.elapsed() < timeout,
            "condition did not settle within {timeout:?}"
        );
        thread::sleep(Duration::from_millis(40));
    }
}

#[derive(Debug)]
pub(crate) struct Stock {
    pub(crate) root: PathBuf,
    pub(crate) herdr: PathBuf,
    pub(crate) client: Client,
    pub(crate) server: Process,
}

impl Stock {
    pub(crate) fn start() -> Result<Self> {
        let herdr = fs::canonicalize(
            std::env::var_os("HERDR_NAV_TEST_BINARY")
                .context("set HERDR_NAV_TEST_BINARY to a stock Herdr 0.9.1+ binary")?,
        )?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = PathBuf::from(format!("/tmp/nav-stock-{}-{nonce}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        let root = fs::canonicalize(root)?;
        let config = root.join("config/herdr");
        fs::create_dir_all(&config)?;
        fs::write(config.join("config.toml"), CONFIG)?;
        let plugin = root.join("plugin");
        fs::create_dir_all(plugin.join("bin"))?;
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("herdr-plugin.toml"),
            plugin.join("herdr-plugin.toml"),
        )?;
        fs::copy(
            env!("CARGO_BIN_EXE_herdr-nav"),
            plugin.join("bin/herdr-nav"),
        )?;
        let log = File::create(root.join("server.log"))?;
        let mut command = command(&root, &herdr);
        command
            .arg("server")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        let mut stock = Self {
            client: Client::new(root.join("api.sock")),
            root,
            herdr,
            server: Process(command.spawn()?),
        };
        eventually(Duration::from_secs(5), || {
            ensure!(
                stock.server.0.try_wait()?.is_none(),
                "isolated server exited"
            );
            Ok(stock.client.endpoint().exists().then_some(()))
        })?;
        stock.client.ping()?;
        Ok(stock)
    }

    pub(crate) fn rpc(
        &self,
        method: &'static str,
        params: &Value,
        tag: &'static str,
    ) -> Result<Value> {
        Ok(herdrkit::testing::request(
            &self.client,
            method,
            params,
            tag,
        )?)
    }

    pub(crate) fn link(&self) -> Result<()> {
        let linked = self.rpc(
            "plugin.link",
            &json!({"path":self.root.join("plugin")}),
            "plugin_linked",
        )?;
        let warnings = linked.pointer("/plugin/warnings").and_then(Value::as_array);
        ensure!(
            warnings.is_none_or(Vec::is_empty),
            "plugin registration warnings: {warnings:?}"
        );
        Ok(())
    }

    pub(crate) fn action_command(&self) -> Command {
        let mut cmd = command(&self.root, &self.root.join("plugin/bin/herdr-nav"));
        cmd.env("HERDR_PLUGIN_ID", "herdr-nav")
            .env("HERDR_PLUGIN_ROOT", self.root.join("plugin"))
            .env(
                "HERDR_PLUGIN_CONFIG_DIR",
                self.root.join("config/herdr/plugins/config/herdr-nav"),
            )
            .env(
                "HERDR_PLUGIN_STATE_DIR",
                self.root.join("state/herdr/plugins/herdr-nav"),
            )
            .env("HERDR_PLUGIN_CONTEXT_JSON", "{}");
        cmd
    }

    pub(crate) fn action(&self, name: &str) -> Result<String> {
        let mut cmd = self.action_command();
        cmd.env("HERDR_PLUGIN_ACTION_ID", name);
        run(cmd)
    }

    pub(crate) fn history(&self) -> Result<History> {
        let status: Status = serde_json::from_str(&self.action("status")?)?;
        ensure!(
            status.last_error.is_none(),
            "worker error: {:?}",
            status.last_error
        );
        Ok(status.navigation.history)
    }

    pub(crate) fn focus(&self) -> Result<PaneId> {
        self.client
            .navigation_snapshot()?
            .focused_pane_id
            .context("no focused pane")
    }

    pub(crate) fn focus_hooks(&self) -> Result<Vec<String>> {
        let reply = self.rpc(
            "plugin.log.list",
            &json!({"plugin_id":"herdr-nav","limit":200}),
            "plugin_log_list",
        )?;
        let logs = reply
            .get("logs")
            .and_then(Value::as_array)
            .context("missing plugin logs")?;
        Ok(logs
            .iter()
            .filter(|log| log.get("event").and_then(Value::as_str) == Some("pane.focused"))
            .filter_map(|log| log.get("log_id").and_then(Value::as_str).map(str::to_owned))
            .collect())
    }

    pub(crate) fn visit(&self, pane: &PaneId, terminal: &TerminalId) -> Result<()> {
        self.client.pane_focus(pane)?;
        self.wait_current(terminal)
    }

    pub(crate) fn wait_current(&self, terminal: &TerminalId) -> Result<()> {
        eventually(Duration::from_secs(8), || {
            Ok((self.history()?.current.as_ref() == Some(terminal)).then_some(()))
        })
    }

    pub(crate) fn record_path(&self) -> Result<PathBuf> {
        let services = self.root.join("state/herdr/plugins/herdr-nav/services");
        let entry = fs::read_dir(services)?
            .next()
            .context("no service directory")??;
        Ok(entry.path())
    }

    pub(crate) fn wait_worker_exit(&self, timeout: Duration) -> Result<()> {
        let path = self.record_path()?.join("history/record.lock");
        eventually(timeout, || {
            let file = File::open(&path)?;
            match file.try_lock() {
                Ok(()) => Ok(Some(())),
                Err(std::fs::TryLockError::WouldBlock) => Ok(None),
                Err(error) => Err(error.into()),
            }
        })
    }

    pub(crate) fn restart_worker(&self) -> Result<()> {
        self.rpc(
            "plugin.disable",
            &json!({"plugin_id":"herdr-nav"}),
            "plugin_disabled",
        )?;
        self.wait_worker_exit(Duration::from_secs(15))?;
        let last: u64 = fs::read_to_string(self.record_path()?.join("launch-attempt"))?.parse()?;
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        thread::sleep(Duration::from_secs((last + 31).saturating_sub(now)));
        self.rpc(
            "plugin.enable",
            &json!({"plugin_id":"herdr-nav"}),
            "plugin_enabled",
        )?;
        Ok(())
    }

    pub(crate) fn shutdown(&mut self) -> Result<()> {
        // Disabling first allows an immediate owner check to exit instead of
        // waiting through the transient-server-error grace period.
        self.rpc(
            "plugin.disable",
            &json!({"plugin_id":"herdr-nav"}),
            "plugin_disabled",
        )?;
        self.wait_worker_exit(Duration::from_secs(15))?;
        self.rpc("server.stop", &json!({}), "ok")?;
        eventually(Duration::from_secs(5), || {
            Ok(self.server.0.try_wait()?.map(|_| ()))
        })?;
        Ok(())
    }
}
impl Drop for Stock {
    fn drop(&mut self) {
        let stopped = self.shutdown();
        if let Err(error) = stopped {
            eprintln!(
                "stock test cleanup: {error:#}; artifacts at {}",
                self.root.display()
            );
            let _ = self.server.0.kill();
            let _ = self.server.0.wait();
            let _ = self.wait_worker_exit(Duration::from_secs(50));
        }
        if std::thread::panicking() {
            eprintln!("stock test artifacts: {}", self.root.display());
        } else {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

pub(crate) fn command(root: &Path, program: &Path) -> Command {
    let mut command = Command::new(program);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("HERDR_") || name.starts_with("HERDRKIT_") || name.starts_with("XDG_") {
            command.env_remove(key);
        }
    }
    command
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("HERDR_CONFIG_PATH", root.join("config/herdr/config.toml"))
        .env("HERDR_SOCKET_PATH", root.join("api.sock"))
        .env("HERDR_SESSION", "nav-test")
        .env("SHELL", "/bin/sh")
        .env("TERM", "xterm-256color");
    command
}

pub(crate) fn run(command: Command) -> Result<String> {
    let result = dots_process::output(command, Duration::from_secs(25), 64 * 1024)?;
    ensure!(
        result.status.success(),
        "action failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?)
}
