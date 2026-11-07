//! The per-machine sync agent and one-shot sync passes.
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result};
use dots_substrate::{Flush, Maintenance, Pass, PassReport, Schedule, Substrate, Trigger};
use serde_json::json;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;

use crate::command::SyncCommand;
use crate::substrate::unix;

/// How long shutdown waits for the pass in progress before exiting anyway.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(15);

pub(crate) fn run(command: &SyncCommand) -> Result<String> {
    match *command {
        SyncCommand::Once => {
            let substrate = crate::substrate::open()?;
            let now = SystemTime::now();
            let (pass, shared) = crate::share::pass(&substrate, Trigger::Explicit, now)?;
            let maintenance = substrate.maintain()?;
            let mut value = report(&pass);
            value["tasks"] = crate::share::report(shared.as_ref());
            value["maintenance"] = match maintenance {
                Maintenance::Skipped => json!(null),
                Maintenance::Ran(summary) => json!(summary),
            };
            value["sync"] = crate::substrate::health(&substrate.health()?);
            Ok(format!("{}\n", serde_json::to_string_pretty(&value)?))
        }
        SyncCommand::Run { interval, resync } => {
            let schedule = Schedule {
                interval: Duration::from_secs(interval),
                resync: Duration::from_secs(resync),
            };
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("start the sync runtime")?
                .block_on(agent(schedule))?;
            Ok(String::new())
        }
    }
}

fn report(pass: &Pass) -> serde_json::Value {
    match pass {
        Pass::Busy => json!({"pass": "busy"}),
        Pass::Backoff { retry_at } => json!({"pass": "backoff", "retry_at": unix(*retry_at)}),
        Pass::Done(PassReport {
            offline,
            fetched,
            flush,
        }) => {
            let flush = match flush {
                Flush::Idle => json!({"state": "idle"}),
                Flush::Skipped => json!({"state": "skipped"}),
                Flush::Throttled { retry_at } => {
                    json!({"state": "throttled", "retry_at": unix(*retry_at)})
                }
                Flush::Sent(sent) => json!({
                    "state": "sent",
                    "applied": sent.applied.iter().map(|(name, _)| name).collect::<Vec<_>>(),
                    "stranded": sent.stranded,
                    "retained": sent.retained,
                    "error": sent.error,
                }),
            };
            json!({
                "pass": if offline.is_some() { "offline" } else { "done" },
                "error": offline,
                "fetched": fetched,
                "flush": flush,
            })
        }
    }
}

/// Log one line to stderr, which the service manager keeps.
fn log(message: &str) {
    eprintln!("dots sync: {message}");
}

/// Run passes as they come due until a termination signal.
async fn agent(schedule: Schedule) -> Result<()> {
    let mut terminate = signal(SignalKind::terminate()).context("handle SIGTERM")?;
    let mut interrupt = signal(SignalKind::interrupt()).context("handle SIGINT")?;
    let mut hangup = signal(SignalKind::hangup()).context("handle SIGHUP")?;
    let root = crate::substrate::root()?;

    // Idle until configured; `substrate init` needs no agent restart.
    let mut announced = false;
    while !crate::substrate::configured()? {
        if !announced {
            log(&format!(
                "no substrate is configured under {}; idling",
                root.display()
            ));
            announced = true;
        }
        tokio::select! {
            _ = terminate.recv() => return Ok(()),
            _ = interrupt.recv() => return Ok(()),
            _ = hangup.recv() => return Ok(()),
            () = tokio::time::sleep(schedule.interval) => {}
        }
    }

    let substrate = Arc::new(crate::substrate::open()?);
    let (wake, mut woken) = mpsc::channel(1);
    let _watcher = watch_changes(&substrate.changes(), wake)?;
    log(&format!(
        "running for machine {} every {}s",
        substrate.machine(),
        schedule.interval.as_secs()
    ));
    loop {
        substrate
            .check_schema()
            .context("the substrate state changed under the agent; restart it")?;
        let now = SystemTime::now();
        if substrate.due(schedule, now)? {
            let pass = Arc::clone(&substrate);
            let mut running = tokio::task::spawn_blocking(move || step(&pass, now));
            tokio::select! {
                finished = &mut running => finished.context("sync pass panicked")??,
                () = stopping(&mut terminate, &mut interrupt, &mut hangup) => {
                    // Passes commit each step atomically; waiting only lets
                    // the current network request finish.
                    let _ = tokio::time::timeout(SHUTDOWN_GRACE, running).await;
                    return Ok(());
                }
            }
        }
        let sleep = next_wake(&substrate, schedule, SystemTime::now())?;
        tokio::select! {
            () = stopping(&mut terminate, &mut interrupt, &mut hangup) => return Ok(()),
            () = tokio::time::sleep(sleep) => {}
            Some(()) = woken.recv() => {}
        }
    }
}

async fn stopping(
    terminate: &mut tokio::signal::unix::Signal,
    interrupt: &mut tokio::signal::unix::Signal,
    hangup: &mut tokio::signal::unix::Signal,
) {
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
        _ = hangup.recv() => {}
    }
}

/// One scheduled pass with task replication, then Git upkeep when the pass
/// moved data.
fn step(substrate: &Substrate, now: SystemTime) -> Result<()> {
    let pass = crate::share::pass(substrate, Trigger::Scheduled, now);
    if let Ok((_, Some((_, imported)))) = &pass {
        for (root, reason) in &imported.errors {
            log(&format!("import of task tree {root} failed: {reason}"));
        }
        for root in &imported.lost {
            log(&format!(
                "task tree {root} was taken over elsewhere before this machine pushed its last changes"
            ));
        }
    }
    let pass = pass.map(|(pass, _)| pass);
    match pass {
        Ok(Pass::Done(report)) => {
            if let Some(reason) = &report.offline {
                log(&format!("remote unavailable: {reason}"));
            }
            let moved = !report.fetched.is_empty() || matches!(report.flush, Flush::Sent(_));
            if moved && let Maintenance::Ran(summary) = substrate.maintain()? {
                log(&summary);
            }
            Ok(())
        }
        Ok(Pass::Busy | Pass::Backoff { .. }) => Ok(()),
        // A schema change is fatal; other failures are recorded and backed off.
        Err(error) if schema_changed(&error) => Err(error),
        Err(error) => {
            log(&format!("pass failed: {error:#}"));
            Ok(())
        }
    }
}

/// Whether a pass failed because another binary changed the substrate schema.
fn schema_changed(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<dots_substrate::Error>(),
            Some(dots_substrate::Error::Schema { .. })
        ) || matches!(
            cause.downcast_ref::<dots_tasks::Error>(),
            Some(dots_tasks::Error::Substrate(
                dots_substrate::Error::Schema { .. }
            ))
        )
    })
}

/// Until the schedule's next pass or the end of a backoff, at least a second.
fn next_wake(substrate: &Substrate, schedule: Schedule, now: SystemTime) -> Result<Duration> {
    let health = substrate.health()?;
    let next = match health.failing.and_then(|failing| failing.retry_at) {
        Some(retry_at) => retry_at,
        None => health
            .pass_started_at
            .map_or(now, |started| started + schedule.interval),
    };
    Ok(next
        .duration_since(now)
        .unwrap_or_default()
        .clamp(Duration::from_secs(1), schedule.interval))
}

/// Wake the agent whenever a writer touches the changes marker.
fn watch_changes(marker: &Path, wake: mpsc::Sender<()>) -> Result<notify::RecommendedWatcher> {
    use notify::Watcher as _;

    let directory = marker
        .parent()
        .context("the changes marker needs a directory")?;
    let name = marker.file_name().map(ToOwned::to_owned);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let relevant = event.is_ok_and(|event| {
            event
                .paths
                .iter()
                .any(|path| path.file_name() == name.as_deref())
        });
        if relevant {
            // A full channel already holds a wake-up.
            let _ = wake.try_send(());
        }
    })
    .context("start the file watcher")?;
    watcher
        .watch(directory, notify::RecursiveMode::NonRecursive)
        .with_context(|| format!("watch {}", directory.display()))?;
    Ok(watcher)
}
