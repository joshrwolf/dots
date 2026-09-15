use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail, ensure};
use serde_json::json;

use crate::Issue;

const HELP: &str = "Usage:
  herdr-issue codex
  herdr-issue claude
  herdr-issue configure-linear --session NAME --herdr /absolute/path/to/herdr
  herdr-issue linear --session NAME --herdr /absolute/path/to/herdr
  herdr-issue open --repo PATH --issue ENG-123 --branch BRANCH [--uuid UUID] [--prompt-stdin] [--session NAME] [--no-focus]
  herdr-issue context --repo PATH --branch BRANCH [--session NAME]
  herdr-issue unbind --repo PATH --branch BRANCH [--session NAME]

Open and context inherit the calling Herdr session unless --session is supplied.
Linear uses LINEAR_WORK_DIR, LINEAR_ISSUE_IDENTIFIER, LINEAR_ISSUE_BRANCH_NAME,
and LINEAR_PROMPT. Configure-linear preserves other coding-tools.json settings.
Open returns JSON with workspace_id, tab_id, pane_id, checkout and state_database.
Context returns the stored issue and prompt as JSON.
Unbind removes only local issue context, for explicit branch/issue reconciliation. No Linear API is called.
An external target session must already be running; start it with herdr --session NAME.
";

#[derive(Debug)]
struct Arguments {
    command: String,
    values: BTreeMap<String, String>,
}

impl Arguments {
    fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Self> {
        let command = arguments.next().unwrap_or_else(|| "--help".into());
        let allowed: &[&str] = match command.as_str() {
            "configure-linear" | "linear" => &["--session", "--herdr"],
            "open" => &[
                "--repo",
                "--issue",
                "--branch",
                "--uuid",
                "--prompt-stdin",
                "--session",
                "--no-focus",
            ],
            "context" | "unbind" => &["--repo", "--branch", "--session"],
            "--help" | "-h" | "codex" | "claude" => &[],
            _ => bail!("unknown command {command}; run herdr-issue --help"),
        };
        let mut values = BTreeMap::new();
        while let Some(flag) = arguments.next() {
            ensure!(
                allowed.contains(&flag.as_str()),
                "unknown option {flag} for {command}"
            );
            ensure!(!values.contains_key(&flag), "duplicate option {flag}");
            let value = if ["--no-focus", "--prompt-stdin"].contains(&flag.as_str()) {
                String::new()
            } else {
                let value = arguments
                    .next()
                    .with_context(|| format!("missing value for {flag}"))?;
                ensure!(
                    !value.is_empty() && !value.starts_with("--"),
                    "missing value for {flag}"
                );
                value
            };
            values.insert(flag, value);
        }
        Ok(Self { command, values })
    }

    fn get(&self, flag: &str) -> Option<&str> {
        self.values.get(flag).map(String::as_str)
    }
    fn required(&self, flag: &str) -> Result<&str> {
        self.get(flag)
            .with_context(|| format!("{flag} is required"))
    }
    fn has(&self, flag: &str) -> bool {
        self.values.contains_key(flag)
    }
}

pub fn run() -> Result<()> {
    let arguments = Arguments::parse(std::env::args().skip(1))?;
    if ["--help", "-h"].contains(&arguments.command.as_str()) {
        print!("{HELP}");
        return Ok(());
    }
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is required")?);
    ensure!(home.is_absolute(), "HOME must be absolute");
    if arguments.command == "configure-linear" {
        let session = arguments.required("--session")?;
        // Reuse normalized session validation without inspecting or starting it.
        herdrkit::connection::Request::new(Some(session.to_owned()), None, None)?;
        let herdr = Path::new(arguments.required("--herdr")?);
        ensure!(
            herdr.is_absolute()
                && herdr.is_file()
                && fs::metadata(herdr)?.permissions().mode() & 0o111 != 0,
            "--herdr must name an absolute executable file"
        );
        configure(&home, &std::env::current_exe()?, session, herdr)?;
        println!(
            "Configured Linear. Use Work on issue → Custom script and choose a repository directory."
        );
        return Ok(());
    }
    if ["codex", "claude"].contains(&arguments.command.as_str()) {
        return launch_agent(&home, &arguments.command);
    }
    let linear = arguments.command == "linear";
    let repo = if linear {
        required_env("LINEAR_WORK_DIR")?
    } else {
        arguments.required("--repo")?.to_owned()
    };
    let repo = fs::canonicalize(&repo).context("resolving selected directory")?;
    ensure!(repo.is_dir(), "selected repository must be a directory");
    let branch = if linear {
        required_env("LINEAR_ISSUE_BRANCH_NAME")?
    } else {
        arguments.required("--branch")?.to_owned()
    };
    let issue = if ["context", "unbind"].contains(&arguments.command.as_str()) {
        None
    } else {
        let identifier = if linear {
            required_env("LINEAR_ISSUE_IDENTIFIER")?
        } else {
            arguments.required("--issue")?.to_owned()
        };
        let prompt = if linear {
            optional_env("LINEAR_PROMPT")?
        } else if arguments.has("--prompt-stdin") {
            let mut prompt = String::new();
            std::io::stdin()
                .take(1024 * 1024 + 1)
                .read_to_string(&mut prompt)
                .context("reading issue prompt from stdin")?;
            Some(prompt)
        } else {
            None
        };
        Some(Issue::new(
            identifier,
            branch.clone(),
            arguments.get("--uuid").map(str::to_owned),
            prompt,
        )?)
    };
    let session = if linear {
        Some(arguments.required("--session")?)
    } else {
        arguments.get("--session")
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut request = herdrkit::connection::Request::from_env(session)?;
    if linear {
        request = request.with_executable(PathBuf::from(arguments.required("--herdr")?))?;
    }
    let connection = runtime.block_on(request.resolve())?;
    let state_dir = home.join(".local/state/herdr-issue");
    if arguments.command == "unbind" {
        crate::unbind(connection.client(), &state_dir, &repo, &branch)?;
        println!("{{\"unbound\":true}}");
    } else if let Some(issue) = issue {
        let opened = crate::open(
            connection.client(),
            &state_dir,
            &repo,
            &issue,
            !arguments.has("--no-focus"),
        )?;
        println!("{}", serde_json::to_string(&opened)?);
    } else {
        let context = crate::context(connection.client(), &state_dir, &repo, &branch)?;
        println!("{}", serde_json::to_string(&context)?);
    }
    Ok(())
}

fn launch_agent(home: &Path, kind: &str) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let connection = runtime.block_on(herdrkit::connection::resolve(None))?;
    let mut command = crate::agent::command(
        connection.client(),
        &home.join(".local/state/herdr-issue"),
        &std::env::current_dir()?,
        kind,
    )?;
    // Deliberately interactive: the agent replaces this launcher in the
    // user's shell pane, retaining its terminal and native configuration.
    Err(command.exec()).context("starting the issue agent")
}

fn optional_env(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {name}")),
    }
}

fn required_env(name: &str) -> Result<String> {
    let value = optional_env(name)?.with_context(|| format!("Linear did not supply {name}"))?;
    ensure!(!value.is_empty(), "Linear supplied an empty {name}");
    Ok(value)
}

fn configure(home: &Path, executable: &Path, session: &str, herdr: &Path) -> Result<()> {
    let directory = home.join(".linear");
    fs::create_dir_all(&directory).context("creating Linear configuration directory")?;
    let path = directory.join("coding-tools.json");
    let mut config: serde_json::Value = match fs::read(&path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).context("reading existing Linear configuration")?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => return Err(error).context("reading Linear configuration"),
    };
    let object = config
        .as_object_mut()
        .context("Linear configuration must be a JSON object")?;
    object.insert("openIssue".into(), json!({
        "path": executable,
        "args": ["linear", "--session", session, "--herdr", herdr],
        "env": ["LINEAR_WORK_DIR", "LINEAR_ISSUE_IDENTIFIER", "LINEAR_ISSUE_BRANCH_NAME", "LINEAR_PROMPT"]
    }));
    let temporary = directory.join(format!(".coding-tools.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        serde_json::to_writer_pretty(&mut file, &config)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, &path).context("replacing Linear custom-script configuration")?;
        std::fs::File::open(&directory)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_duplicate_and_missing_options() {
        for args in [
            vec!["open", "--repo"],
            vec!["linear", "--no-focus"],
            vec!["open", "--branch", "a", "--branch", "b"],
            vec!["context", "--branch", "--repo"],
        ] {
            assert!(Arguments::parse(args.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn config_preserves_other_settings_and_passes_context_as_environment_data() {
        let home = std::env::temp_dir().join(format!("herdr-issue-config-{}", std::process::id()));
        fs::create_dir_all(home.join(".linear")).unwrap();
        fs::write(
            home.join(".linear/coding-tools.json"),
            r#"{"anotherTool":{"enabled":true}}"#,
        )
        .unwrap();
        configure(
            &home,
            Path::new("/opt/bin/herdr-issue"),
            "development",
            Path::new("/opt/bin/herdr"),
        )
        .unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(home.join(".linear/coding-tools.json")).unwrap())
                .unwrap();
        assert_eq!(value.get("anotherTool"), Some(&json!({"enabled":true})));
        assert_eq!(
            value.pointer("/openIssue/args"),
            Some(&json!([
                "linear",
                "--session",
                "development",
                "--herdr",
                "/opt/bin/herdr"
            ]))
        );
        assert_eq!(
            value.pointer("/openIssue/env"),
            Some(&json!([
                "LINEAR_WORK_DIR",
                "LINEAR_ISSUE_IDENTIFIER",
                "LINEAR_ISSUE_BRANCH_NAME",
                "LINEAR_PROMPT"
            ]))
        );
        fs::remove_dir_all(home).unwrap();
    }
}
