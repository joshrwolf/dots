//! `dots github watch`: checkout resolution, storage location, presentation,
//! and the follow loop a picker keeps open.
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::Path;
use std::process::Output;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use github_watch::{Ci, Entry, NewEntry, Refresh, Review, Store};
use serde::Serialize;
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

use crate::command::WatchCommand;

/// Bounds of a `follow` pause: never a busy loop, and new entries appear
/// within a minute.
const FOLLOW_FLOOR: Duration = Duration::from_secs(1);
const FOLLOW_CAP: Duration = Duration::from_secs(60);

pub(crate) struct Request<'request> {
    pub command: WatchCommand,
    /// The SQLite file; its directory is created on first use.
    pub store: &'request Path,
    pub gh: &'request dyn github_watch::Gh,
    pub directory: &'request Path,
    pub session: Option<&'request str>,
    /// The Rex session the work belongs to.
    pub rex_session: Option<&'request str>,
    /// Current Unix time in seconds.
    pub clock: &'request dyn Fn() -> i64,
    pub json: bool,
    /// Where `follow` streams its lines; other commands return their output.
    pub out: &'request mut dyn Write,
    /// Waits about the given time between `follow` passes; `false` means the
    /// reader is gone and `follow` ends.
    pub sleep: &'request mut dyn FnMut(Duration) -> bool,
}

/// The invocation's `gh` transport, run from its working directory.
pub(crate) struct Gh<'transport> {
    pub transport: &'transport dyn github_inspect::Transport,
    pub directory: &'transport Path,
}

impl github_watch::Gh for Gh<'_> {
    fn gh(&self, args: &[&str], timeout: Duration) -> Result<Output, github_watch::TransportError> {
        self.transport.gh(self.directory, args, timeout)
    }
}

/// The list JSON contract consumed by the Rex picker view.
#[derive(Debug, Serialize)]
struct Pr<'entry> {
    url: &'entry str,
    repository: String,
    number: i64,
    title: Option<&'entry str>,
    state: github_watch::State,
    review: github_watch::Review,
    ci: github_watch::Ci,
    head_ref: Option<&'entry str>,
    checkout: Option<&'entry str>,
    added_by: Option<&'entry str>,
    rex_session: Option<&'entry str>,
    added_at: i64,
    checked_at: Option<i64>,
    error: Option<&'entry str>,
}

impl<'entry> From<&'entry Entry> for Pr<'entry> {
    fn from(entry: &'entry Entry) -> Self {
        Self {
            url: entry.url.as_str(),
            repository: format!("{}/{}", entry.owner, entry.repo),
            number: entry.number,
            title: entry.title.as_deref(),
            state: entry.state,
            review: entry.review,
            ci: entry.ci,
            head_ref: entry.head_ref.as_deref(),
            checkout: entry.checkout.as_deref(),
            added_by: entry.added_by.as_deref(),
            rex_session: entry.rex_session.as_deref(),
            added_at: entry.added_at,
            checked_at: entry.checked_at,
            error: entry.error.as_deref(),
        }
    }
}

/// `show --json`: the list entry plus its cached description.
#[derive(Debug, Serialize)]
struct Shown<'entry> {
    #[serde(flatten)]
    pr: Pr<'entry>,
    body: Option<&'entry str>,
}

pub(crate) fn run(request: &mut Request<'_>) -> Result<String> {
    if let Some(directory) = request.store.parent() {
        std::fs::create_dir_all(directory).context("create PR watch store directory")?;
    }
    let mut store = Store::open(request.store)?;
    let now = (request.clock)();
    match &request.command {
        WatchCommand::Add { url, checkout } => {
            let checkout = checkout_path(request.directory, checkout.as_deref())?;
            store.add(
                &NewEntry {
                    url,
                    added_by: request.session,
                    checkout: Some(&checkout),
                    rex_session: request.rex_session,
                },
                now,
            )?;
            let refresh = store.refresh(request.gh, now, Some(url))?;
            let entry = store
                .get(url)?
                .context("the added PR was removed concurrently")?;
            let warnings: Vec<_> = refresh
                .failures
                .iter()
                .map(|failure| {
                    format!("added, but GitHub status is unavailable: {}", failure.error)
                })
                .collect();
            if request.json {
                return Ok(crate::json::render(serde_json::json!({
                    "pr": Pr::from(&entry),
                    "warnings": warnings,
                })));
            }
            let mut out = line(&entry);
            for warning in &warnings {
                let _ = writeln!(out, "Warning: {warning}");
            }
            Ok(crate::render::printable(&out))
        }
        WatchCommand::Remove { url } => {
            let removed = store.remove(url)?;
            if request.json {
                return Ok(crate::json::render(
                    serde_json::json!({ "url": url, "removed": removed }),
                ));
            }
            Ok(if removed {
                format!("Removed {}\n", url.as_str())
            } else {
                format!("{} was not on the watch list\n", url.as_str())
            })
        }
        WatchCommand::List => {
            let listing = store.list(now)?;
            if request.json {
                let prs: Vec<_> = listing.entries.iter().map(Pr::from).collect();
                return Ok(crate::json::render(
                    serde_json::json!({ "prs": prs, "due": listing.due }),
                ));
            }
            let mut out: String = listing.entries.iter().map(line).collect();
            if listing.entries.is_empty() {
                out.push_str("No watched PRs.\n");
            }
            if listing.due > 0 {
                let _ = writeln!(
                    out,
                    "{} due for a check; run dots github watch refresh",
                    listing.due
                );
            }
            Ok(crate::render::printable(&out))
        }
        WatchCommand::Show { url, color } => shown(&mut store, url, request.json, *color, now),
        WatchCommand::Follow => {
            follow(
                &mut store,
                request.gh,
                request.clock,
                request.out,
                request.sleep,
            )?;
            Ok(String::new())
        }
        WatchCommand::Refresh { all } => {
            if *all {
                store.make_due(now)?;
            }
            let refresh = store.refresh(request.gh, now, None)?;
            if request.json {
                return Ok(crate::json::render(serde_json::to_value(&refresh)?));
            }
            Ok(crate::render::printable(&summary(&refresh)))
        }
    }
}

/// An absolute, resolved checkout path; relative paths start at `directory`.
fn checkout_path(directory: &Path, checkout: Option<&Path>) -> Result<String> {
    let path = checkout.map_or_else(|| directory.to_path_buf(), |path| directory.join(path));
    let path = path
        .canonicalize()
        .with_context(|| format!("resolve checkout {}", path.display()))?;
    path.to_str()
        .map(ToOwned::to_owned)
        .context("checkout path is not Unicode")
}

fn line(entry: &Entry) -> String {
    let mut out = format!(
        "{:<7} {:<17} ci:{:<8} {}/{}#{}  {}",
        entry.state.as_str(),
        entry.review.as_str(),
        entry.ci.as_str(),
        entry.owner,
        entry.repo,
        entry.number,
        entry.title.as_deref().unwrap_or("(not checked yet)")
    );
    if let Some(error) = &entry.error {
        let _ = write!(
            out,
            "  [error: {}]",
            error.lines().next().unwrap_or_default()
        );
    }
    out.push('\n');
    out
}

/// `show`: one cached entry, without touching the network.
fn shown(
    store: &mut Store,
    url: &github_watch::PullRequestUrl,
    json: bool,
    color: bool,
    now: i64,
) -> Result<String> {
    let entry = store.get(url)?.with_context(|| {
        format!(
            "{} is not on the watch list; add it with dots github watch add",
            url.as_str()
        )
    })?;
    if json {
        return Ok(crate::json::render(serde_json::json!({
            "pr": Shown { pr: Pr::from(&entry), body: entry.body.as_deref() },
        })));
    }
    Ok(show(
        &entry,
        now,
        color,
        columns(std::env::var("COLUMNS").ok().as_deref()),
    ))
}

/// Text for a preview pane; reads only the cache.
fn show(entry: &Entry, now: i64, color: bool, width: Option<usize>) -> String {
    // Each logical line carries its own styles. Wrap before emitting SGR so
    // breaks cannot split an escape, and reapply styles after each line reset.
    let mut lines: Vec<Vec<(String, &'static str)>> = vec![vec![
        (
            format!("{}/{}#{}", entry.owner, entry.repo, entry.number),
            "1",
        ),
        ("  ".into(), ""),
        (
            entry.title.as_deref().unwrap_or("(not checked yet)").into(),
            "1",
        ),
    ]];
    let review = match entry.review {
        Review::Approved => ("approved", "32"),
        Review::ChangesRequested => ("changes requested", "31"),
        Review::ReviewRequired => ("review required", "33"),
        Review::None => ("no review decision", "2"),
        Review::Unknown => ("review unknown", "2"),
    };
    let ci = match entry.ci {
        Ci::Success => ("CI success", "32"),
        Ci::Failure => ("CI failure", "31"),
        Ci::Pending => ("CI pending", "33"),
        Ci::None => ("no CI checks", "2"),
        Ci::Unknown => ("CI unknown", "2"),
    };
    let state = match entry.state {
        github_watch::State::Open => "32",
        github_watch::State::Draft | github_watch::State::Unknown => "2",
        github_watch::State::Merged => "35",
        github_watch::State::Closed => "31",
    };
    lines.push(vec![
        (entry.state.as_str().into(), state),
        (" · ".into(), "2"),
        (review.0.into(), review.1),
        (" · ".into(), "2"),
        (ci.0.into(), ci.1),
    ]);
    if let Some(branch) = &entry.head_ref {
        lines.push(vec![("Branch: ".into(), "2"), (branch.clone(), "")]);
    }
    if let Some(checkout) = &entry.checkout {
        lines.push(vec![("Checkout: ".into(), "2"), (checkout.clone(), "")]);
    }
    if let Some(session) = &entry.rex_session {
        lines.push(vec![("Session: ".into(), "2"), (session.clone(), "")]);
    }
    let added = ago(entry.added_at, now);
    lines.push(vec![(
        match &entry.added_by {
            Some(session) => format!("Added {added} by {session}"),
            None => format!("Added {added}"),
        },
        "2",
    )]);
    lines.push(vec![(
        match entry.checked_at {
            Some(checked) => format!("Checked {}", ago(checked, now)),
            None => "Not checked yet".into(),
        },
        "2",
    )]);
    if let Some(error) = &entry.error {
        lines.push(vec![(format!("Last check failed: {error}"), "31")]);
    }
    if color {
        lines.push(vec![("─".repeat(width.unwrap_or(40)), "2")]);
    } else {
        lines.push(Vec::new());
    }
    match entry.body.as_deref().filter(|body| !body.trim().is_empty()) {
        Some(body) => {
            for line in body.split('\n') {
                lines.push(vec![(
                    line.into(),
                    if line.starts_with('#') { "1" } else { "" },
                )]);
            }
        }
        None => lines.push(vec![("No description.".into(), "")]),
    }
    render_show(lines, color, width)
}

fn columns(value: Option<&str>) -> Option<usize> {
    value?.parse::<usize>().ok().filter(|width| *width > 0)
}

/// A styled grapheme, never an ANSI escape sequence.
type Cluster = (String, &'static str);

fn render_show(lines: Vec<Vec<Cluster>>, color: bool, width: Option<usize>) -> String {
    let mut out = String::new();
    for line in lines {
        let mut current = Vec::new();
        let mut cells = 0;
        for (text, style) in line {
            let printable = crate::render::printable(&text);
            for grapheme in printable.graphemes(true) {
                if grapheme == "\n" {
                    emit_line(&mut out, &current, color);
                    current.clear();
                    cells = 0;
                    continue;
                }
                let size = grapheme.width();
                if width.is_some_and(|width| cells + size > width) && !current.is_empty() {
                    // Prefer the last word boundary. The whitespace itself is
                    // omitted only at a wrap; original spacing stays otherwise.
                    let break_at = current
                        .iter()
                        .rposition(|(g, _): &Cluster| g.chars().all(char::is_whitespace));
                    if let Some(at) = break_at {
                        let mut next = current.split_off(at + 1);
                        current.pop();
                        while current
                            .last()
                            .is_some_and(|(g, _)| g.chars().all(char::is_whitespace))
                        {
                            current.pop();
                        }
                        emit_line(&mut out, &current, color);
                        while next
                            .first()
                            .is_some_and(|(g, _)| g.chars().all(char::is_whitespace))
                        {
                            next.remove(0);
                        }
                        current = next;
                        cells = current.iter().map(|(g, _)| g.width()).sum();
                    } else {
                        emit_line(&mut out, &current, color);
                        current.clear();
                        cells = 0;
                    }
                }
                // A word can be wider than the pane; split it at grapheme
                // boundaries. A single wide grapheme cannot fit a 1-cell pane.
                if width.is_some_and(|width| cells + size > width) && !current.is_empty() {
                    emit_line(&mut out, &current, color);
                    current.clear();
                    cells = 0;
                }
                current.push((grapheme.to_owned(), style));
                cells += size;
            }
        }
        emit_line(&mut out, &current, color);
    }
    out
}

fn emit_line(out: &mut String, clusters: &[Cluster], color: bool) {
    let mut active = "";
    for (grapheme, style) in clusters {
        if color && active != *style {
            // Reset first: SGR attributes such as dim or bold otherwise carry over.
            let _ = if style.is_empty() {
                write!(out, "\u{1b}[0m")
            } else {
                write!(out, "\u{1b}[0;{style}m")
            };
            active = style;
        }
        out.push_str(grapheme);
    }
    if color {
        out.push_str("\u{1b}[0m");
    }
    out.push('\n');
}

/// How long before `now` `then` was, in its largest whole unit.
fn ago(then: i64, now: i64) -> String {
    match now.saturating_sub(then) {
        ..=0 => "just now".to_owned(),
        seconds @ 1..60 => format!("{seconds}s ago"),
        seconds @ 60..3600 => format!("{}m ago", seconds / 60),
        seconds @ 3600..86_400 => format!("{}h ago", seconds / 3600),
        seconds => format!("{}d ago", seconds / 86_400),
    }
}

/// Print `ready`, then refresh due PRs until the reader goes away, printing
/// `changed` whenever the rows differ from those last seen.
///
/// Rows compare with the time and outcome of their last check, so every
/// completed check prints `changed` and a preview's check age stays current,
/// as does an entry another session added, removed, or refreshed. Only the
/// schedule is ignored: rescheduling alone changes nothing a picker shows.
fn follow(
    store: &mut Store,
    gh: &dyn github_watch::Gh,
    clock: &dyn Fn() -> i64,
    out: &mut dyn Write,
    sleep: &mut dyn FnMut(Duration) -> bool,
) -> Result<()> {
    if !signal(out, "ready")? {
        return Ok(());
    }
    let mut seen = rows(store, clock())?;
    loop {
        let now = clock();
        store.refresh(gh, now, None)?;
        let current = rows(store, now)?;
        if current != seen {
            if !signal(out, "changed")? {
                return Ok(());
            }
            seen = current;
        }
        if !sleep(pause(store.next_due()?, clock())) {
            return Ok(());
        }
    }
}

/// Write one line; `false` once the reader has closed its end.
fn signal(out: &mut dyn Write, line: &str) -> Result<bool> {
    match writeln!(out, "{line}").and_then(|()| out.flush()) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(false),
        Err(error) => Err(error).context("write follow output"),
    }
}

/// The rows as a picker sees them, without their schedule.
fn rows(store: &mut Store, now: i64) -> Result<Vec<Entry>> {
    Ok(store
        .list(now)?
        .entries
        .into_iter()
        .map(|mut entry| {
            entry.next_check_at = None;
            entry
        })
        .collect())
}

/// Until the earliest due PR, within [`FOLLOW_FLOOR`] and [`FOLLOW_CAP`].
fn pause(next_due: Option<i64>, now: i64) -> Duration {
    next_due.map_or(FOLLOW_CAP, |due| {
        Duration::from_secs(u64::try_from(due.saturating_sub(now)).unwrap_or_default())
            .clamp(FOLLOW_FLOOR, FOLLOW_CAP)
    })
}

/// Sleep for `duration` unless stdout's reader closes first; `false` once it has.
///
/// Polling stdout for a hang-up notices a closed pipe without writing to it.
/// macOS reports `POLLHUP` only alongside a requested event, so this asks for
/// `POLLPRI`, which a pipe's write end never raises otherwise. Where stdout
/// reports something else or cannot be polled, this sleeps.
pub(crate) fn sleep_while_read(duration: Duration) -> bool {
    use nix::errno::Errno;
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    use std::os::fd::AsFd as _;

    let deadline = Instant::now() + duration;
    let stdout = io::stdout();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        let timeout = PollTimeout::try_from(left).unwrap_or(PollTimeout::MAX);
        let mut fds = [PollFd::new(stdout.as_fd(), PollFlags::POLLPRI)];
        match poll(&mut fds, timeout) {
            Ok(0) | Err(Errno::EINTR) => {}
            Ok(_) => {
                let events = fds.first().and_then(nix::poll::PollFd::revents);
                if events.is_some_and(|events| {
                    events.intersects(PollFlags::POLLHUP | PollFlags::POLLERR)
                }) {
                    return false;
                }
                std::thread::sleep(left);
            }
            Err(_) => std::thread::sleep(left),
        }
    }
}

/// Exit 0 on SIGTERM, SIGINT, or SIGHUP: a reader stopping `follow` is its
/// normal end. Call before starting threads, which inherit the blocked mask;
/// child processes start with an empty one.
pub(crate) fn exit_on_signals() -> Result<()> {
    use nix::sys::signal::{SigSet, Signal};

    let mut signals = SigSet::empty();
    for signal in [Signal::SIGTERM, Signal::SIGINT, Signal::SIGHUP] {
        signals.add(signal);
    }
    signals
        .thread_block()
        .context("block termination signals")?;
    std::thread::Builder::new()
        .name("signals".into())
        .spawn(move || {
            if signals.wait().is_ok() {
                std::process::exit(0);
            }
            // Cannot wait: let the signals take their default action here.
            let _ = signals.thread_unblock();
            loop {
                std::thread::park();
            }
        })
        .context("start the signal thread")?;
    Ok(())
}

fn summary(refresh: &Refresh) -> String {
    if refresh.refreshed + refresh.failed == 0 {
        return "No PRs were due.\n".to_owned();
    }
    let mut out = format!(
        "Refreshed {} PRs; {} failed.\n",
        refresh.refreshed, refresh.failed
    );
    for failure in &refresh.failures {
        let _ = writeln!(out, "Failed: {} {}", failure.url, failure.error);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt as _;
    use std::path::PathBuf;
    use std::process::ExitStatus;
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::{Value, json};

    use super::*;

    const NOW: i64 = 1_700_000_000;

    fn scratch() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "dots-watch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(directory.join("checkout")).unwrap();
        directory.canonicalize().unwrap()
    }

    /// Replies with one neutral open PR, or fails like an unauthenticated `gh`.
    struct Reply(bool);

    impl github_watch::Gh for Reply {
        fn gh(
            &self,
            _args: &[&str],
            _timeout: Duration,
        ) -> Result<Output, github_watch::TransportError> {
            let (code, stdout, stderr) = if self.0 {
                let body = json!({"data": {"p0": {"pullRequest": {
                    "title": "Improve the example", "body": "Explains the change.", "state": "OPEN", "isDraft": false,
                    "reviewDecision": "APPROVED", "headRefName": "feature/example",
                    "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}
                }}}});
                (0, body.to_string(), "")
            } else {
                (4, String::new(), "gh: authentication required")
            };
            Ok(Output {
                status: ExitStatus::from_raw(code << 8),
                stdout: stdout.into_bytes(),
                stderr: stderr.as_bytes().to_vec(),
            })
        }
    }

    fn run_with(root: &Path, gh: &dyn github_watch::Gh, command: WatchCommand) -> Value {
        serde_json::from_str(&run_at(root, gh, command, true).unwrap()).unwrap()
    }

    fn run_at(
        root: &Path,
        gh: &dyn github_watch::Gh,
        command: WatchCommand,
        json: bool,
    ) -> Result<String> {
        let store = root.join("state").join(github_watch::STORE_PATH);
        let directory = root.join("checkout");
        run(&mut Request {
            command,
            store: &store,
            gh,
            directory: &directory,
            session: Some("pi-session"),
            rex_session: Some("session:work"),
            clock: &|| NOW,
            json,
            out: &mut Vec::new(),
            sleep: &mut |_| false,
        })
    }

    fn url(number: u64) -> github_watch::PullRequestUrl {
        github_watch::PullRequestUrl::parse(&format!(
            "https://github.com/example/project/pull/{number}"
        ))
        .unwrap()
    }

    #[test]
    fn list_json_is_the_picker_contract() {
        let root = scratch();
        let added = run_with(
            &root,
            &Reply(true),
            WatchCommand::Add {
                url: url(7),
                checkout: None,
            },
        );
        assert_eq!(added.pointer("/warnings"), Some(&json!([])));
        let failed = run_with(
            &root,
            &Reply(false),
            WatchCommand::Add {
                url: url(8),
                checkout: Some("../checkout".into()),
            },
        );
        assert!(
            failed
                .pointer("/warnings/0")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("authentication required"),
            "{failed}"
        );

        let listed = run_with(&root, &Reply(true), WatchCommand::List);
        let checkout = root.join("checkout").to_str().unwrap().to_owned();
        assert_eq!(
            listed,
            json!({
                "prs": [
                    {
                        "url": "https://github.com/example/project/pull/8",
                        "repository": "example/project",
                        "number": 8,
                        "title": null,
                        "state": "unknown",
                        "review": "unknown",
                        "ci": "unknown",
                        "head_ref": null,
                        "checkout": checkout,
                        "added_by": "pi-session",
                        "rex_session": "session:work",
                        "added_at": NOW,
                        "checked_at": null,
                        "error": "gh api graphql failed: gh: authentication required"
                    },
                    {
                        "url": "https://github.com/example/project/pull/7",
                        "repository": "example/project",
                        "number": 7,
                        "title": "Improve the example",
                        "state": "open",
                        "review": "approved",
                        "ci": "success",
                        "head_ref": "feature/example",
                        "checkout": checkout,
                        "added_by": "pi-session",
                        "rex_session": "session:work",
                        "added_at": NOW,
                        "checked_at": NOW,
                        "error": null
                    }
                ],
                "due": 0
            })
        );

        let removed = run_with(&root, &Reply(true), WatchCommand::Remove { url: url(8) });
        assert_eq!(removed, json!({"url": url(8).as_str(), "removed": true}));
        let refreshed = run_with(&root, &Reply(true), WatchCommand::Refresh { all: true });
        assert_eq!(
            refreshed,
            json!({"refreshed": 1, "failed": 0, "failures": []})
        );
    }

    #[test]
    fn readable_list_is_one_line_per_pr() {
        let mut store = Store::open(&scratch().join("watch.sqlite3")).unwrap();
        let entry = store
            .add(
                &NewEntry {
                    url: &url(7),
                    added_by: None,
                    checkout: None,
                    rex_session: None,
                },
                NOW,
            )
            .unwrap();
        assert_eq!(
            line(&entry),
            "unknown unknown           ci:unknown  example/project#7  (not checked yet)\n"
        );
    }

    #[test]
    fn show_json_is_the_list_entry_with_its_description() {
        let root = scratch();
        run_with(
            &root,
            &Reply(true),
            WatchCommand::Add {
                url: url(7),
                checkout: None,
            },
        );
        let shown = run_with(
            &root,
            &Reply(false),
            WatchCommand::Show {
                url: url(7),
                color: true,
            },
        );
        let listed = run_with(&root, &Reply(false), WatchCommand::List);
        let mut expected = listed.pointer("/prs/0").unwrap().clone();
        expected
            .as_object_mut()
            .unwrap()
            .insert("body".into(), json!("Explains the change."));
        assert_eq!(shown, json!({ "pr": expected }));

        let text = run_at(
            &root,
            &Reply(false),
            WatchCommand::Show {
                url: url(7),
                color: false,
            },
            false,
        )
        .unwrap();
        assert!(text.ends_with("\n\nExplains the change.\n"), "{text}");

        let unknown = run_at(
            &root,
            &Reply(true),
            WatchCommand::Show {
                url: url(9),
                color: false,
            },
            false,
        )
        .unwrap_err()
        .to_string();
        assert_eq!(
            unknown,
            "https://github.com/example/project/pull/9 is not on the watch list; add it with dots github watch add"
        );
    }

    fn entry() -> Entry {
        Entry {
            url: url(7),
            owner: "example".into(),
            repo: "project".into(),
            number: 7,
            added_at: NOW - 2 * 3600 - 5,
            added_by: Some("pi-session".into()),
            checkout: Some("/work/project".into()),
            rex_session: Some("session:work".into()),
            title: Some("Improve the example".into()),
            state: github_watch::State::Open,
            review: Review::ChangesRequested,
            ci: Ci::Pending,
            head_ref: Some("feature/example".into()),
            body: Some(
                "Explains the change.\n\n- caf\u{e9} detail\n- \u{1b}[31mred\u{1b}[0m".into(),
            ),
            checked_at: Some(NOW - 3 * 60 - 10),
            error: None,
            next_check_at: Some(NOW + 60),
        }
    }

    #[test]
    fn show_is_plain_text_for_a_preview() {
        assert_eq!(
            show(&entry(), NOW, false, None),
            "example/project#7  Improve the example\n\
             open \u{b7} changes requested \u{b7} CI pending\n\
             Branch: feature/example\n\
             Checkout: /work/project\n\
             Session: session:work\n\
             Added 2h ago by pi-session\n\
             Checked 3m ago\n\
             \n\
             Explains the change.\n\n- caf\u{e9} detail\n- red\n"
        );

        let unchecked = Entry {
            added_by: None,
            title: None,
            state: github_watch::State::Unknown,
            review: Review::Unknown,
            ci: Ci::Unknown,
            head_ref: None,
            body: None,
            checked_at: None,
            added_at: NOW,
            ..entry()
        };
        assert_eq!(
            show(&unchecked, NOW, false, None),
            "example/project#7  (not checked yet)\n\
             unknown \u{b7} review unknown \u{b7} CI unknown\n\
             Checkout: /work/project\n\
             Session: session:work\n\
             Added just now\n\
             Not checked yet\n\
             \n\
             No description.\n"
        );

        let failed = Entry {
            review: Review::None,
            ci: Ci::None,
            checkout: None,
            body: Some(String::new()),
            checked_at: Some(NOW - 3 * 86_400),
            error: Some("gh api graphql failed: gh: authentication required".into()),
            ..entry()
        };
        assert_eq!(
            show(&failed, NOW, false, None),
            "example/project#7  Improve the example\n\
             open \u{b7} no review decision \u{b7} no CI checks\n\
             Branch: feature/example\n\
             Session: session:work\n\
             Added 2h ago by pi-session\n\
             Checked 3d ago\n\
             Last check failed: gh api graphql failed: gh: authentication required\n\
             \n\
             No description.\n"
        );
    }

    #[test]
    fn color_marks_statuses_and_metadata_without_touching_json() {
        let mut pr = entry();
        pr.error = Some("gh unavailable".into());
        pr.body = Some("# Heading\ntext\n## Detail".into());
        let cases = [
            (github_watch::State::Open, "32"),
            (github_watch::State::Draft, "2"),
            (github_watch::State::Merged, "35"),
            (github_watch::State::Closed, "31"),
            (github_watch::State::Unknown, "2"),
        ];
        for (state, sgr) in cases {
            pr.state = state;
            let colored = show(&pr, NOW, true, Some(25));
            assert!(
                colored.contains(&format!("\u{1b}[0;{sgr}m{}", state.as_str())),
                "{colored:?}"
            );
            assert!(
                colored.lines().all(|line| line.ends_with("\u{1b}[0m")),
                "{colored:?}"
            );
        }
        pr.state = github_watch::State::Open;
        for (review, label, sgr) in [
            (Review::Approved, "approved", "32"),
            (Review::ChangesRequested, "changes requested", "31"),
            (Review::ReviewRequired, "review required", "33"),
            (Review::None, "no review decision", "2"),
            (Review::Unknown, "review unknown", "2"),
        ] {
            pr.review = review;
            let output = show(&pr, NOW, true, None);
            if sgr == "2" {
                assert!(
                    output.contains(&format!("\u{1b}[0;2m · {label}")),
                    "{output:?}"
                );
            } else {
                assert!(
                    output.contains(&format!("\u{1b}[0;{sgr}m{label}")),
                    "{output:?}"
                );
            }
        }
        pr.review = Review::Approved;
        for (ci, label, sgr) in [
            (Ci::Success, "CI success", "32"),
            (Ci::Failure, "CI failure", "31"),
            (Ci::Pending, "CI pending", "33"),
            (Ci::None, "no CI checks", "2"),
            (Ci::Unknown, "CI unknown", "2"),
        ] {
            pr.ci = ci;
            let output = show(&pr, NOW, true, None);
            if sgr == "2" {
                assert!(
                    output.contains(&format!("\u{1b}[0;2m · {label}")),
                    "{output:?}"
                );
            } else {
                assert!(
                    output.contains(&format!("\u{1b}[0;{sgr}m{label}")),
                    "{output:?}"
                );
            }
        }
        let colored = show(&pr, NOW, true, None);
        assert!(
            colored.starts_with(
                "\u{1b}[0;1mexample/project#7\u{1b}[0m  \u{1b}[0;1mImprove the example"
            ),
            "{colored:?}"
        );
        for label in [
            "Branch: ",
            "Checkout: ",
            "Session: ",
            "Added 2h ago",
            "Checked 3m ago",
        ] {
            assert!(
                colored.contains(&format!("\u{1b}[0;2m{label}")),
                "{colored:?}"
            );
        }
        assert!(colored.contains("\u{1b}[0;31mLast check failed: gh unavailable"));
        assert!(colored.contains(&format!("\u{1b}[0;2m{}\u{1b}[0m\n", "─".repeat(40))));
        assert!(
            colored.contains("\u{1b}[0;1m# Heading\u{1b}[0m\ntext\u{1b}[0m\n\u{1b}[0;1m## Detail")
        );
        assert!(colored.contains("\u{1b}[0;2m · "));
    }

    #[test]
    fn wraps_on_visible_word_and_grapheme_boundaries() {
        let mut pr = entry();
        pr.title = Some("abcde fghij".into());
        pr.body = Some("cafe\u{301} 一 👩\u{200d}💻 longunbrokenword".into());
        for color in [false, true] {
            let output = show(&pr, NOW, color, Some(12));
            let plain = crate::render::printable(&output);
            assert!(plain.contains("abcde fghij"), "{plain:?}");
            assert!(
                plain.contains("cafe\u{301} 一 👩\u{200d}💻\nlongunbroken\nword"),
                "{plain:?}"
            );
            assert!(plain.lines().all(|line| line.width() <= 12), "{plain:?}");
            if color {
                assert!(output.lines().all(|line| line.ends_with("\u{1b}[0m")));
                assert!(output.contains("\u{1b}[0;1mabcde fghij"));
                assert!(output.contains("\u{1b}[0;2m─"));
            } else {
                assert!(!output.contains('\u{1b}'));
            }
        }
    }

    #[test]
    fn invalid_columns_leave_plain_text_unwrapped() {
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("-8"),
            Some("x"),
            Some("9999999999999999999999999999"),
        ] {
            assert_eq!(columns(value), None);
        }
        assert_eq!(columns(Some("17")), Some(17));
        assert!(
            show(&entry(), NOW, false, columns(Some("nope")))
                .starts_with("example/project#7  Improve the example\n")
        );
    }

    #[test]
    fn relative_times_use_the_largest_whole_unit() {
        for (seconds, expected) in [
            (-5, "just now"),
            (0, "just now"),
            (59, "59s ago"),
            (60, "1m ago"),
            (3599, "59m ago"),
            (3600, "1h ago"),
            (86_399, "23h ago"),
            (86_400, "1d ago"),
        ] {
            assert_eq!(ago(NOW - seconds, NOW), expected, "{seconds}");
        }
    }

    /// One open PR per call, from a script; an exhausted script fails the call.
    struct Replies(std::cell::RefCell<Vec<Option<&'static str>>>);

    impl Replies {
        /// `Some(ci)` answers with that rollup; `None` fails like an unauthenticated `gh`.
        fn new(mut script: Vec<Option<&'static str>>) -> Self {
            script.reverse();
            Self(std::cell::RefCell::new(script))
        }

        fn left(&self) -> usize {
            self.0.borrow().len()
        }
    }

    impl github_watch::Gh for Replies {
        fn gh(
            &self,
            _args: &[&str],
            _timeout: Duration,
        ) -> Result<Output, github_watch::TransportError> {
            let (code, stdout, stderr) = match self
                .0
                .borrow_mut()
                .pop()
                .expect("unexpected gh call")
            {
                Some(ci) => {
                    let body = json!({"data": {"p0": {"pullRequest": {
                        "title": "Improve the example", "body": "", "state": "OPEN", "isDraft": false,
                        "reviewDecision": null, "headRefName": "feature/example",
                        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": ci}}}]}
                    }}}});
                    (0, body.to_string(), "")
                }
                None => (4, String::new(), "gh: authentication required"),
            };
            Ok(Output {
                status: ExitStatus::from_raw(code << 8),
                stdout: stdout.into_bytes(),
                stderr: stderr.as_bytes().to_vec(),
            })
        }
    }

    /// Shared output that closes, like a reader exiting, after `lines` lines.
    #[derive(Clone)]
    struct Reader {
        text: std::rc::Rc<std::cell::RefCell<String>>,
        lines: usize,
    }

    impl Reader {
        fn new(lines: usize) -> Self {
            Self {
                text: std::rc::Rc::default(),
                lines,
            }
        }

        fn lines(&self) -> Vec<String> {
            self.text.borrow().lines().map(ToOwned::to_owned).collect()
        }
    }

    impl Write for Reader {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.text.borrow().matches('\n').count() >= self.lines {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            self.text
                .borrow_mut()
                .push_str(std::str::from_utf8(bytes).unwrap());
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Run `follow` over a fake clock that sleeps advance; `wake` picks how
    /// far each sleep advances it (`None` stops following), from the requested
    /// pause and the lines read so far.
    fn follow_with(
        store: &mut Store,
        gh: &dyn github_watch::Gh,
        reader: &Reader,
        mut wake: impl FnMut(Duration, &[String]) -> Option<Duration>,
    ) -> Vec<Duration> {
        let clock = std::cell::Cell::new(NOW);
        let mut pauses = Vec::new();
        let mut sleep = |pause: Duration| {
            pauses.push(pause);
            match wake(pause, &reader.lines()) {
                Some(advance) => {
                    clock.set(clock.get() + i64::try_from(advance.as_secs()).unwrap());
                    true
                }
                None => false,
            }
        };
        follow(store, gh, &|| clock.get(), &mut reader.clone(), &mut sleep).unwrap();
        pauses
    }

    fn watched(numbers: &[u64]) -> (PathBuf, Store) {
        let path = scratch().join("watch.sqlite3");
        let mut store = Store::open(&path).unwrap();
        for number in numbers {
            store
                .add(
                    &NewEntry {
                        url: &url(*number),
                        added_by: None,
                        checkout: None,
                        rex_session: None,
                    },
                    NOW,
                )
                .unwrap();
        }
        (path, store)
    }

    #[test]
    fn follow_prints_changed_after_each_completed_check() {
        let (_, mut store) = watched(&[7]);
        let gh = Replies::new(vec![
            Some("PENDING"),
            Some("PENDING"),
            None,
            Some("SUCCESS"),
        ]);
        let reader = Reader::new(usize::MAX);
        let mut seen = Vec::new();
        let pauses = follow_with(&mut store, &gh, &reader, |pause, lines| {
            seen.push(lines.len());
            (seen.len() < 4).then_some(pause)
        });
        // First check: new status. Second: the same status, checked later,
        // so a preview's check age moves. Third: a recorded failure, due
        // again after 300s, past the cap: the fourth pass changes nothing.
        assert_eq!(reader.lines(), ["ready", "changed", "changed", "changed"]);
        assert_eq!(seen, [2, 3, 4, 4]);
        assert_eq!(pauses, [FOLLOW_CAP; 4]);
        assert_eq!(gh.left(), 1, "nothing was due on the fourth pass");
    }

    #[test]
    fn follow_sleeps_until_the_earliest_due_check() {
        for (next_due, expected) in [
            (None, FOLLOW_CAP),
            (Some(NOW - 30), FOLLOW_FLOOR),
            (Some(NOW), FOLLOW_FLOOR),
            (Some(NOW + 1), Duration::from_secs(1)),
            (Some(NOW + 25), Duration::from_secs(25)),
            (Some(NOW + 60), FOLLOW_CAP),
            (Some(NOW + 600), FOLLOW_CAP),
        ] {
            assert_eq!(pause(next_due, NOW), expected, "{next_due:?}");
        }

        // Settled CI is due again in 600s; waking 590s later leaves 10.
        let (_, mut store) = watched(&[7]);
        let gh = Replies::new(vec![Some("SUCCESS")]);
        let mut wakes = [Duration::from_secs(590)].into_iter();
        let pauses = follow_with(&mut store, &gh, &Reader::new(usize::MAX), |_, _| {
            wakes.next()
        });
        assert_eq!(pauses, [FOLLOW_CAP, Duration::from_secs(10)]);

        // Nothing will ever be due: re-check at the cap for new entries.
        let (_, mut store) = watched(&[]);
        let mut count = 0;
        let pauses = follow_with(
            &mut store,
            &Replies::new(vec![]),
            &Reader::new(usize::MAX),
            |pause, _| {
                count += 1;
                (count < 2).then_some(pause)
            },
        );
        assert_eq!(pauses, [FOLLOW_CAP; 2]);
    }

    #[test]
    fn follow_reports_entries_other_sessions_change() {
        let (path, mut store) = watched(&[7]);
        let gh = Replies::new(vec![Some("SUCCESS")]);
        let reader = Reader::new(usize::MAX);
        let mut other = Store::open(&path).unwrap();
        let mut passes = 0;
        follow_with(&mut store, &gh, &reader, |pause, _| {
            passes += 1;
            match passes {
                1 => {
                    other.remove(&url(7)).unwrap();
                    Some(pause)
                }
                _ => None,
            }
        });
        assert_eq!(reader.lines(), ["ready", "changed", "changed"]);
    }

    #[test]
    fn follow_ends_cleanly_when_its_reader_closes() {
        let (_, mut store) = watched(&[7]);
        let gh = Replies::new(vec![Some("SUCCESS")]);
        let pauses = follow_with(&mut store, &gh, &Reader::new(0), |pause, _| Some(pause));
        assert_eq!((pauses.len(), gh.left()), (0, 1), "closed before ready");

        let reader = Reader::new(1);
        let pauses = follow_with(&mut store, &gh, &reader, |pause, _| Some(pause));
        assert_eq!(reader.lines(), ["ready"]);
        assert_eq!((pauses.len(), gh.left()), (0, 0), "closed before changed");
    }
}
