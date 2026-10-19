//! Exercises herdrkit against the herdr server that launched it.
//!
//! `cargo run -p herdrkit --example live`, from inside a herdr session.
//!
//! This is not a test and cannot be one: it asserts nothing, because what it is
//! for is finding out what herdr actually does. Every surprising number in
//! `events`' documentation — the ~100ms cadence, the replayed backlog, the
//! focus subscriptions that never go quiet — came from running this and reading
//! the output. A fake server would have agreed with whatever was assumed.

use std::time::{Duration, Instant};

use herdrkit::api::{AgentRef, AgentStatus, ReadSource, Sound};
use herdrkit::events::{Events, Subscription};
use herdrkit::{Client, MetadataReporter, Result, Tokens};

fn main() -> Result<()> {
    let client = Client::from_env()?;
    let pong = client.ping()?;
    println!("herdr {} protocol {}\n", pong.version, pong.protocol);

    let snapshot = client.snapshot()?;
    let (Some(workspace), Some(pane)) = (
        snapshot.workspaces.first().map(|w| w.workspace_id.clone()),
        snapshot.panes.first().map(|p| p.pane_id.clone()),
    ) else {
        println!("no workspace or pane to probe");
        return Ok(());
    };

    println!("-- subscribe returns at once; the first batch carries the replay --");
    let opened = Instant::now();
    let mut events = Events::subscribe(&client, &Subscription::workspace_topology())?;
    println!("   subscribed in {:?}", opened.elapsed());
    let waited = Instant::now();
    let replay = events.changes(Duration::from_secs(5))?;
    println!(
        "   first batch: {} frame(s) in {:?}",
        replay.as_ref().map_or(0, Vec::len),
        waited.elapsed()
    );

    println!("-- a quiet interval is None, not an error --");
    let waited = Instant::now();
    let quiet = events.changes(Duration::from_millis(400))?;
    println!(
        "   {:?} after {:?}",
        quiet.as_ref().map(Vec::len),
        waited.elapsed()
    );

    // `ci` rather than a name of its own: that is the token the sidebar config
    // draws, so this is visible confirmation rather than a call that returned
    // ok. It expires on its own, so nothing has to clean up after the probe.
    println!("-- tokens, with a ttl so a crash fades out instead of lying --");
    let tokens = Tokens::new().set("ci", "✓ live probe")?;
    MetadataReporter::new(&client, "herdrkit-live", Duration::from_secs(10))?
        .report_workspace(&workspace, &tokens)?;
    println!("   pushed {tokens:?} to {workspace} — watch its sidebar row for 10s");

    println!("-- a notification reports whether it was actually shown --");
    println!(
        "   {:?}",
        client.notify("herdrkit", Some("live probe"), Sound::None)?
    );

    println!("-- pane.read --");
    let read = client.pane_read(&pane, ReadSource::Visible, Some(2))?;
    println!(
        "   {} revision {} truncated {}",
        read.pane_id, read.revision, read.truncated
    );

    println!("-- a blocking wait must outlive the 3s socket deadline --");
    let target = snapshot
        .agents
        .first()
        .map_or(pane.clone(), |agent| agent.pane_id.clone());
    let waited = Instant::now();
    let outcome = client.agent_wait(
        AgentRef::Pane(&target),
        &[AgentStatus::Done],
        Some(std::time::Duration::from_secs(5)),
    );
    match outcome {
        Ok(matched) => println!(
            "   reached {:?} after {:?}",
            matched.agent_status,
            waited.elapsed()
        ),
        Err(error) => println!("   after {:?}: {error}", waited.elapsed()),
    }
    Ok(())
}
