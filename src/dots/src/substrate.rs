//! This machine's substrate configuration and state.
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail, ensure};
use dots_documents::staged_file::StagedFile;
use dots_substrate::git::GitBackend;
use dots_substrate::{Config, Health, Substrate};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::command::SubstrateCommand;

const CONFIG: &str = "config.json";
const MAX_CONFIG_BYTES: usize = 64 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    remote: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential_helper: Option<String>,
}

/// `$XDG_DATA_HOME/dots/substrate`: configuration, Git cache and substrate state.
pub(crate) fn root() -> Result<PathBuf> {
    Ok(crate::project::data_root()?.join("substrate"))
}

/// Whether this machine has a substrate configuration.
pub(crate) fn configured() -> Result<bool> {
    let path = root()?.join(CONFIG);
    path.try_exists()
        .with_context(|| format!("inspect {}", path.display()))
}

fn settings() -> Result<Settings> {
    let path = root()?.join(CONFIG);
    let bytes = dots_documents::text::read_bytes(&path, MAX_CONFIG_BYTES)?.with_context(|| {
        format!(
            "no substrate is configured at {}; run dots substrate init --remote URL",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

/// Open this machine's substrate over its configured remote.
pub(crate) fn open() -> Result<Substrate> {
    let settings = settings()?;
    let root = root()?;
    let mut backend = GitBackend::open(&root.join("git"), settings.remote)?;
    if let Some(helper) = settings.credential_helper {
        // Git reads no user configuration, so this is the only helper; it reads
        // its own configuration from the real home.
        backend = backend
            .config("credential.helper", helper)
            .env("HOME", crate::xdg::home()?);
    }
    Ok(Substrate::open(
        &root,
        Box::new(backend),
        Config::default(),
    )?)
}

pub(crate) fn run(command: SubstrateCommand) -> Result<String> {
    match command {
        SubstrateCommand::Init {
            remote,
            credential_helper,
        } => {
            ensure!(
                !remote.is_empty()
                    && !remote.starts_with('-')
                    && !remote.contains(char::is_whitespace),
                "remote must be a Git URL"
            );
            ensure!(
                credential_helper
                    .as_ref()
                    .is_none_or(|helper| !helper.trim().is_empty()),
                "credential helper must not be empty"
            );
            let settings = Settings {
                remote,
                credential_helper,
            };
            let root = root()?;
            let path = root.join(CONFIG);
            if dots_documents::text::read_bytes(&path, MAX_CONFIG_BYTES)?.is_some() {
                let current = self::settings()?;
                if current != settings {
                    bail!(
                        "{} already names remote {}; refusing to change it",
                        path.display(),
                        current.remote
                    );
                }
            } else {
                dots_documents::directories::create_within(&root, &crate::project::data_root()?)?;
                let staged = StagedFile::create(&root)?;
                staged
                    .file()
                    .write_all(&serde_json::to_vec_pretty(&settings)?)
                    .context("write substrate configuration")?;
                staged.publish_new(&path)?;
            }
            encode(
                &json!({"config": path, "remote": settings.remote, "credential_helper": settings.credential_helper}),
            )
        }
        SubstrateCommand::Status => {
            let settings = settings()?;
            let substrate = open()?;
            let status = substrate.status()?;
            let pending: Vec<_> = status
                .pending
                .iter()
                .map(|p| json!({"ref": p.name, "root": p.root, "since": unix(p.since)}))
                .collect();
            let stranded: Vec<_> = status
                .stranded
                .iter()
                .map(|s| json!({"ref": s.name, "root": s.root, "expected": s.expected, "actual": s.actual, "at": unix(s.at)}))
                .collect();
            let counters = status.counters;
            encode(&json!({
                "remote": settings.remote,
                "credential_helper": settings.credential_helper,
                "machine": substrate.machine(),
                "sync": health(&status.health),
                "pending": pending,
                "stranded": stranded,
                "last_flush": status.last_flush.map(|f| json!({"at": unix(f.at), "error": f.error})),
                "counters": {
                    "passes": counters.passes,
                    "failed_passes": counters.failed_passes,
                    "lists": counters.lists,
                    "fetches": counters.fetches,
                    "pushes": counters.pushes,
                    "throttled": counters.throttled,
                    "maintenance": counters.maintenance,
                },
            }))
        }
    }
}

/// The last sync pass and, while passes fail, the error and backoff.
pub(crate) fn health(health: &Health) -> serde_json::Value {
    json!({
        "listed_at": health.listed_at.map(unix),
        "last_pass_started_at": health.pass_started_at.map(unix),
        "last_pass_finished_at": health.pass_finished_at.map(unix),
        "failing": health.failing.as_ref().map(|failing| json!({
            "since": unix(failing.since),
            "failures": failing.failures,
            "last_error": failing.reason,
            "retry_at": failing.retry_at.map(unix),
        })),
    })
}

/// Unix seconds, as other dots timestamps.
pub(crate) fn unix(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |age| age.as_secs())
}

fn encode(value: &impl Serialize) -> Result<String> {
    Ok(format!("{}\n", serde_json::to_string_pretty(value)?))
}
