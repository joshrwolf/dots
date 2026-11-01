//! Store behavior over neutral, hand-authored GraphQL replies.
use std::cell::RefCell;
use std::fs;
use std::io;
use std::os::unix::process::ExitStatusExt as _;
use std::path::PathBuf;
use std::process::{ExitStatus, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::*;

const T: i64 = 1_000_000;

/// A directory this test created, removed again on drop. Process IDs repeat
/// and an interrupted run leaves its directory behind, so a name in use is
/// skipped, never reused or removed.
struct Scratch(PathBuf);

static SCRATCH_SERIAL: AtomicU64 = AtomicU64::new(0);

fn scratch_candidate(serial: u64) -> PathBuf {
    std::env::temp_dir().join(format!("github-watch-{}-{serial}", std::process::id()))
}

impl Scratch {
    fn new() -> Self {
        loop {
            let directory = scratch_candidate(SCRATCH_SERIAL.fetch_add(1, Ordering::Relaxed));
            match fs::create_dir(&directory) {
                Ok(()) => return Self(directory),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("create {}: {error}", directory.display()),
            }
        }
    }

    fn store(&self) -> PathBuf {
        self.0.join("watch.sqlite3")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A store in its own scratch directory, which lives as long as the guard.
fn open_store() -> (Scratch, Store) {
    let scratch = Scratch::new();
    let store = Store::open(&scratch.store()).unwrap();
    (scratch, store)
}

#[test]
fn scratch_skips_and_keeps_directories_left_by_an_earlier_process() {
    // Plant a leftover at the first free name the guard has yet to try;
    // every name before it already exists and is left alone.
    let mut serial = SCRATCH_SERIAL.load(Ordering::Relaxed);
    let leftover = loop {
        let directory = scratch_candidate(serial);
        if fs::create_dir(&directory).is_ok() {
            break directory;
        }
        serial += 1;
    };
    fs::write(leftover.join("watch.sqlite3"), "stale").unwrap();

    let scratch = Scratch::new();
    assert_ne!(scratch.0, leftover);
    assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
    drop(scratch);

    assert_eq!(fs::read(leftover.join("watch.sqlite3")).unwrap(), b"stale");
    fs::remove_dir_all(leftover).unwrap();
}

fn url(number: u64) -> PullRequestUrl {
    PullRequestUrl::parse(&format!("https://github.com/example/project/pull/{number}")).unwrap()
}

fn add(store: &mut Store, number: u64, now: i64) -> Entry {
    store
        .add(
            &NewEntry {
                url: &url(number),
                added_by: None,
                checkout: None,
                rex_session: None,
            },
            now,
        )
        .unwrap()
}

fn pull_request(state: &str, draft: bool, review: Option<&str>, rollup: Option<&str>) -> Value {
    json!({
        "pullRequest": {
            "title": "Improve the example",
            "body": "Explains the change.\r\n\r\n- one detail\r\n",
            "state": state,
            "isDraft": draft,
            "reviewDecision": review,
            "headRefName": "feature/example",
            "commits": {"nodes": [{"commit": {"statusCheckRollup": rollup.map(|state| json!({"state": state}))}}]}
        }
    })
}

fn output(code: i32, stdout: impl Into<Vec<u8>>, stderr: &str) -> Output {
    Output {
        status: ExitStatus::from_raw(code << 8),
        stdout: stdout.into(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

fn reply(aliases: &[Value]) -> Output {
    let data: serde_json::Map<_, _> = aliases
        .iter()
        .enumerate()
        .map(|(index, value)| (format!("p{index}"), value.clone()))
        .collect();
    output(0, json!({ "data": data }).to_string(), "")
}

/// Answers each call from a script and records the arguments it received.
#[derive(Default)]
struct Script {
    replies: RefCell<Vec<github_client::Result<Output>>>,
    calls: RefCell<Vec<Vec<String>>>,
}

impl Script {
    fn new(replies: impl IntoIterator<Item = github_client::Result<Output>>) -> Self {
        let mut replies: Vec<_> = replies.into_iter().collect();
        replies.reverse();
        Self {
            replies: RefCell::new(replies),
            calls: RefCell::default(),
        }
    }

    fn calls(&self) -> usize {
        self.calls.borrow().len()
    }
}

impl Gh for Script {
    fn gh(&self, args: &[&str], _timeout: Duration) -> github_client::Result<Output> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(ToString::to_string).collect());
        self.replies.borrow_mut().pop().expect("unexpected gh call")
    }
}

/// Answers any chunk with an open PR per alias.
struct EveryOpen(RefCell<Vec<usize>>);

impl Gh for EveryOpen {
    fn gh(&self, args: &[&str], _timeout: Duration) -> github_client::Result<Output> {
        let count = args.iter().filter(|arg| **arg == "-F").count();
        self.0.borrow_mut().push(count);
        let aliases = vec![pull_request("OPEN", false, None, Some("SUCCESS")); count];
        Ok(reply(&aliases))
    }
}

#[test]
fn add_is_an_idempotent_upsert() {
    let (_scratch, mut store) = open_store();
    let first = store
        .add(
            &NewEntry {
                url: &PullRequestUrl::parse("https://github.com/Example/Project/pull/7/files")
                    .unwrap(),
                added_by: Some("pi-first"),
                checkout: Some("/work/first"),
                rex_session: Some("session:first"),
            },
            T,
        )
        .unwrap();
    assert_eq!(
        first.url.as_str(),
        "https://github.com/example/project/pull/7"
    );
    assert_eq!(
        (first.owner.as_str(), first.repo.as_str(), first.number),
        ("example", "project", 7)
    );
    assert_eq!(
        (first.state, first.review, first.ci),
        (State::Unknown, Review::Unknown, Ci::Unknown)
    );
    assert_eq!(first.next_check_at, Some(T));

    let again = store
        .add(
            &NewEntry {
                url: &url(7),
                added_by: Some("  "),
                checkout: Some("/work/second"),
                rex_session: Some(" "),
            },
            T + 50,
        )
        .unwrap();
    assert_eq!(again.added_at, T);
    assert_eq!(again.added_by.as_deref(), Some("pi-first"));
    assert_eq!(again.checkout.as_deref(), Some("/work/second"));
    assert_eq!(again.rex_session.as_deref(), Some("session:first"));
    let moved = store
        .add(
            &NewEntry {
                url: &url(7),
                added_by: None,
                checkout: None,
                rex_session: Some("session:second"),
            },
            T + 50,
        )
        .unwrap();
    assert_eq!(moved.rex_session.as_deref(), Some("session:second"));
    assert_eq!(moved.checkout.as_deref(), Some("/work/second"));
    assert_eq!(again.next_check_at, Some(T + 50));
    assert_eq!(store.list(T + 50).unwrap().entries.len(), 1);

    let relative = store.add(
        &NewEntry {
            url: &url(8),
            added_by: None,
            checkout: Some("work/relative"),
            rex_session: None,
        },
        T,
    );
    assert!(matches!(relative, Err(Error::Invalid(_))));

    let unstorable =
        PullRequestUrl::parse("https://github.com/example/project/pull/9223372036854775808")
            .unwrap();
    let unstorable = store.add(
        &NewEntry {
            url: &unstorable,
            added_by: None,
            checkout: None,
            rex_session: None,
        },
        T,
    );
    assert!(
        matches!(unstorable, Err(Error::Invalid("pull request number"))),
        "{unstorable:?}"
    );
    assert_eq!(
        serde_json::to_value(&first).unwrap()["url"],
        "https://github.com/example/project/pull/7"
    );
}

#[test]
fn schedule_policy_is_one_table() {
    for (state, ci, expected) in [
        (State::Unknown, Ci::Unknown, Some(T + 60)),
        (State::Open, Ci::Pending, Some(T + 60)),
        (State::Draft, Ci::Pending, Some(T + 60)),
        (State::Open, Ci::Success, Some(T + 600)),
        (State::Draft, Ci::None, Some(T + 600)),
        (State::Open, Ci::Failure, Some(T + 600)),
        (State::Merged, Ci::Pending, None),
        (State::Closed, Ci::Success, None),
    ] {
        assert_eq!(next_check(state, ci, T), expected, "{state:?} {ci:?}");
    }
    assert_eq!(RETRY_SECS, 300, "a failed check is retried in five minutes");
}

#[test]
fn refresh_records_status_and_the_next_due_time() {
    let (_scratch, mut store) = open_store();
    add(&mut store, 7, T);
    let gh = Script::new([Ok(reply(&[pull_request(
        "OPEN",
        true,
        Some("REVIEW_REQUIRED"),
        Some("PENDING"),
    )]))]);
    let refresh = store.refresh(&gh, T, None).unwrap();
    assert_eq!((refresh.refreshed, refresh.failed), (1, 0));
    let entry = store.get(&url(7)).unwrap().unwrap();
    assert_eq!(entry.title.as_deref(), Some("Improve the example"));
    assert_eq!(entry.head_ref.as_deref(), Some("feature/example"));
    assert_eq!(
        (entry.state, entry.review, entry.ci),
        (State::Draft, Review::ReviewRequired, Ci::Pending)
    );
    assert_eq!(
        (entry.checked_at, entry.next_check_at),
        (Some(T), Some(T + 60))
    );

    // Not due again yet: no request at all.
    assert_eq!(store.refresh(&gh, T + 59, None).unwrap().refreshed, 0);
    assert_eq!(gh.calls(), 1);
    let calls = gh.calls.borrow();
    let args = calls.first().unwrap();
    assert_eq!(
        args.get(..4).unwrap(),
        ["api", "graphql", "--hostname", "github.com"]
    );
    assert!(args.contains(&"n0=7".to_owned()));
}

#[test]
fn merged_prs_stop_refreshing_and_are_pruned_a_day_after_observation() {
    let (_scratch, mut store) = open_store();
    add(&mut store, 7, T);
    add(&mut store, 8, T);
    let gh = Script::new([Ok(reply(&[
        pull_request("MERGED", false, Some("APPROVED"), Some("SUCCESS")),
        pull_request("OPEN", false, None, None),
    ]))]);
    store.refresh(&gh, T, None).unwrap();
    let merged = store.get(&url(7)).unwrap().unwrap();
    assert_eq!((merged.state, merged.next_check_at), (State::Merged, None));
    assert_eq!(
        store.make_due(T + 1).unwrap(),
        1,
        "merged rows are not made due"
    );

    let listed = store.list(T + RETAIN_CLOSED_SECS).unwrap();
    assert_eq!(listed.entries.len(), 2);
    let listed = store.list(T + RETAIN_CLOSED_SECS + 1).unwrap();
    assert_eq!(
        listed
            .entries
            .iter()
            .map(|entry| entry.number)
            .collect::<Vec<_>>(),
        [8]
    );
    assert_eq!(listed.due, 1);
}

/// While the first refresher's request is in flight, a second one runs.
struct Concurrent {
    other: RefCell<Store>,
    inner: Script,
    nested: Script,
    observed: RefCell<Option<Result<Refresh, Error>>>,
}

impl Gh for Concurrent {
    fn gh(&self, args: &[&str], timeout: Duration) -> github_client::Result<Output> {
        let nested = self.other.borrow_mut().refresh(&self.nested, T + 1, None);
        *self.observed.borrow_mut() = Some(nested);
        self.inner.gh(args, timeout)
    }
}

#[test]
fn claims_lease_rows_so_concurrent_refreshers_never_double_fetch() {
    let scratch = Scratch::new();
    let path = scratch.store();
    let mut first = Store::open(&path).unwrap();
    let mut second = Store::open(&path).unwrap();
    add(&mut first, 7, T);

    let claimed = first.claim(T, None).unwrap();
    assert_eq!(claimed.len(), 1);
    assert!(second.claim(T, None).unwrap().is_empty());
    // An abandoned lease expires.
    assert_eq!(second.claim(T + LEASE_SECS, None).unwrap().len(), 1);

    add(&mut first, 8, T + 1);
    let gh = Concurrent {
        other: RefCell::new(second),
        inner: Script::new([Ok(reply(&[pull_request("OPEN", false, None, None)]))]),
        nested: Script::new([]),
        observed: RefCell::default(),
    };
    assert_eq!(
        first.refresh(&gh, T + 1, Some(&url(8))).unwrap().refreshed,
        1
    );
    assert_eq!(gh.nested.calls(), 0);
    let observed = gh.observed.borrow();
    let nested = observed.as_ref().unwrap().as_ref().unwrap();
    assert_eq!(nested.refreshed + nested.failed, 0);
}

#[test]
fn a_missing_pr_fails_only_its_row() {
    let (_scratch, mut store) = open_store();
    add(&mut store, 7, T);
    add(&mut store, 8, T);
    add(&mut store, 9, T);
    let body = json!({
        "data": {
            "p0": pull_request("OPEN", false, Some("APPROVED"), Some("SUCCESS")),
            "p1": {"pullRequest": null},
            "p2": null
        },
        "errors": [
            {"type": "NOT_FOUND", "path": ["p1", "pullRequest"], "message": "Could not resolve to a PullRequest with the number of 8."},
            {"type": "NOT_FOUND", "path": ["p2"], "message": "Could not resolve to a Repository with the name 'example/project'."}
        ]
    });
    let gh = Script::new([Ok(output(1, body.to_string(), "gh: Could not resolve"))]);
    let refresh = store.refresh(&gh, T, None).unwrap();
    assert_eq!((refresh.refreshed, refresh.failed), (1, 2));
    let ok = store.get(&url(7)).unwrap().unwrap();
    assert_eq!((ok.state, ok.error), (State::Open, None));
    let missing = store.get(&url(8)).unwrap().unwrap();
    assert_eq!(
        missing.error.as_deref(),
        Some("Could not resolve to a PullRequest with the number of 8.")
    );
    assert_eq!(
        (missing.state, missing.next_check_at),
        (State::Unknown, Some(T + 300))
    );
    assert!(
        store
            .get(&url(9))
            .unwrap()
            .unwrap()
            .error
            .unwrap()
            .contains("Repository")
    );
}

/// Two PRs checked once with settled CI, then due again.
fn settled_pair() -> (Scratch, Store) {
    let (scratch, mut store) = open_store();
    add(&mut store, 7, T);
    add(&mut store, 8, T);
    let open = pull_request("OPEN", false, Some("APPROVED"), Some("SUCCESS"));
    let gh = Script::new([Ok(reply(&[open.clone(), open]))]);
    assert_eq!(store.refresh(&gh, T, None).unwrap().refreshed, 2);
    store.make_due(T + 1).unwrap();
    (scratch, store)
}

/// A failed check keeps the status cached by the check at `T`.
fn assert_failed_keeping_cache(store: &mut Store, number: u64, error: &str) {
    let entry = store.get(&url(number)).unwrap().unwrap();
    assert!(
        entry
            .error
            .as_deref()
            .is_some_and(|text| text.contains(error)),
        "{entry:?}"
    );
    assert_eq!(
        (entry.state, entry.review, entry.ci, entry.checked_at),
        (State::Open, Review::Approved, Ci::Success, Some(T)),
        "{entry:?}"
    );
    assert_eq!(entry.next_check_at, Some(T + 1 + RETRY_SECS));
}

#[test]
fn a_field_error_inside_one_alias_fails_only_that_row() {
    for (field, path) in [
        (
            "/p0/pullRequest/commits/nodes/0/commit/statusCheckRollup",
            json!([
                "p0",
                "pullRequest",
                "commits",
                "nodes",
                0,
                "commit",
                "statusCheckRollup"
            ]),
        ),
        (
            "/p0/pullRequest/reviewDecision",
            json!(["p0", "pullRequest", "reviewDecision"]),
        ),
    ] {
        let (_scratch, mut store) = settled_pair();
        // GraphQL nulls the failed field, which alone would read as "no
        // checks" or "no review decision".
        let mut data = json!({
            "p0": pull_request("OPEN", false, Some("APPROVED"), Some("SUCCESS")),
            "p1": pull_request("OPEN", false, Some("APPROVED"), Some("PENDING")),
        });
        *data.pointer_mut(field).unwrap() = Value::Null;
        let body = json!({
            "data": data,
            "errors": [{"path": path, "message": "Something went wrong while resolving this field."}]
        });
        let gh = Script::new([Ok(output(1, body.to_string(), "gh: Something went wrong"))]);
        let refresh = store.refresh(&gh, T + 1, None).unwrap();
        assert_eq!((refresh.refreshed, refresh.failed), (1, 1), "{field}");
        assert_failed_keeping_cache(&mut store, 7, "Something went wrong while resolving");
        let other = store.get(&url(8)).unwrap().unwrap();
        assert_eq!((other.ci, other.error), (Ci::Pending, None), "{field}");
    }
}

#[test]
fn errors_outside_every_alias_fail_every_row() {
    for path in [json!([]), json!(["p2", "pullRequest"]), json!(["viewer"])] {
        let (_scratch, mut store) = settled_pair();
        let open = pull_request("OPEN", false, Some("APPROVED"), Some("PENDING"));
        let body = json!({
            "data": {"p0": open, "p1": open},
            "errors": [{"path": path, "message": "Request timed out partway."}]
        });
        let gh = Script::new([Ok(output(1, body.to_string(), ""))]);
        let refresh = store.refresh(&gh, T + 1, None).unwrap();
        assert_eq!((refresh.refreshed, refresh.failed), (0, 2), "{path}");
        for number in [7, 8] {
            assert_failed_keeping_cache(&mut store, number, "Request timed out partway.");
        }
    }
}

#[test]
fn a_failed_exit_without_errors_fails_every_row() {
    let (_scratch, mut store) = settled_pair();
    let open = pull_request("OPEN", false, Some("APPROVED"), Some("PENDING"));
    let body = json!({"data": {"p0": open, "p1": open}});
    let gh = Script::new([Ok(output(1, body.to_string(), "gh: HTTP 502"))]);
    let refresh = store.refresh(&gh, T + 1, None).unwrap();
    assert_eq!((refresh.refreshed, refresh.failed), (0, 2));
    for number in [7, 8] {
        assert_failed_keeping_cache(&mut store, number, "gh api graphql failed: gh: HTTP 502");
    }
}

#[test]
fn a_pr_without_its_head_commit_is_a_failed_check() {
    let (_scratch, mut store) = settled_pair();
    let mut headless = pull_request("OPEN", false, Some("APPROVED"), None);
    *headless.pointer_mut("/pullRequest/commits/nodes").unwrap() = json!([]);
    let gh = Script::new([Ok(reply(&[
        headless,
        pull_request("OPEN", false, Some("APPROVED"), None),
    ]))]);
    let refresh = store.refresh(&gh, T + 1, None).unwrap();
    assert_eq!((refresh.refreshed, refresh.failed), (1, 1));
    assert_failed_keeping_cache(&mut store, 7, "without its head commit");
    let no_checks = store.get(&url(8)).unwrap().unwrap();
    assert_eq!(
        (no_checks.ci, no_checks.error),
        (Ci::None, None),
        "a head commit without a rollup has no checks"
    );
}

#[test]
fn a_result_count_mismatch_panics_before_any_row_changes() {
    let (_scratch, mut store) = open_store();
    add(&mut store, 7, T);
    add(&mut store, 8, T);
    let claimed = store.claim(T, None).unwrap();
    assert_eq!(claimed.len(), 2);
    let short = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut outcome = Refresh::default();
        store.record(&claimed, vec![Ok(status(Ci::Success))], T, &mut outcome)
    }));
    assert!(short.is_err(), "a short result list is a defect");
    for number in [7, 8] {
        let entry = store.get(&url(number)).unwrap().unwrap();
        assert_eq!(
            (entry.checked_at, entry.error.as_deref()),
            (None, None),
            "{entry:?}"
        );
    }
    assert!(
        store.claim(T + 1, None).unwrap().is_empty(),
        "leases stay held"
    );
}

#[test]
fn transport_failures_mark_every_claimed_row_and_keep_cached_status() {
    let (_scratch, mut store) = open_store();
    add(&mut store, 7, T);
    add(&mut store, 8, T);
    let open = pull_request("OPEN", false, Some("APPROVED"), Some("SUCCESS"));
    let gh = Script::new([
        Ok(reply(&[open.clone(), open.clone()])),
        Err(github_client::Error::CommandTimedOut {
            program: "gh api graphql".into(),
            timeout: Duration::from_secs(30),
        }),
        Ok(output(1, "", "gh: authentication required")),
        Ok(reply(&[open.clone(), open])),
    ]);
    store.refresh(&gh, T, None).unwrap();

    store.make_due(T + 1).unwrap();
    let refresh = store.refresh(&gh, T + 1, None).unwrap();
    assert_eq!((refresh.refreshed, refresh.failed), (0, 2));
    for number in [7, 8] {
        let entry = store.get(&url(number)).unwrap().unwrap();
        assert!(
            entry.error.as_deref().unwrap().contains("deadline"),
            "{entry:?}"
        );
        assert_eq!(
            (entry.state, entry.ci, entry.checked_at),
            (State::Open, Ci::Success, Some(T))
        );
        assert_eq!(entry.next_check_at, Some(T + 1 + 300));
    }

    let refresh = store.refresh(&gh, T + 301, None).unwrap();
    assert_eq!(refresh.failed, 2);
    assert_eq!(
        refresh.failures.first().unwrap().error,
        "gh api graphql failed: gh: authentication required"
    );

    store.refresh(&gh, T + 601, None).unwrap();
    let entry = store.get(&url(7)).unwrap().unwrap();
    assert_eq!((entry.error, entry.checked_at), (None, Some(T + 601)));
}

#[test]
fn due_rows_are_fetched_in_chunks_of_fifty() {
    let (_scratch, mut store) = open_store();
    for number in 1..=51 {
        add(&mut store, number, T);
    }
    let gh = EveryOpen(RefCell::default());
    assert_eq!(store.refresh(&gh, T, None).unwrap().refreshed, 51);
    assert_eq!(*gh.0.borrow(), [50, 1]);
}

#[test]
fn long_errors_are_bounded_between_graphemes() {
    let text = "e\u{301}".repeat(MAX_ERROR_BYTES);
    let short = bounded(&text);
    assert!(short.len() <= MAX_ERROR_BYTES + '…'.len_utf8());
    assert!(short.trim_end_matches('…').ends_with("e\u{301}"));
}

#[test]
fn descriptions_are_stored_with_lf_endings_and_bounded() {
    let (_scratch, mut store) = open_store();
    add(&mut store, 7, T);
    add(&mut store, 8, T);
    assert_eq!(store.get(&url(7)).unwrap().unwrap().body, None);

    let mut long = pull_request("OPEN", false, None, None);
    let text = format!(
        "{}{}",
        "a".repeat(MAX_BODY_BYTES - 1),
        "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467} tail"
    );
    *long.pointer_mut("/pullRequest/body").unwrap() = json!(text);
    let gh = Script::new([Ok(reply(&[pull_request("OPEN", false, None, None), long]))]);
    store.refresh(&gh, T, None).unwrap();

    assert_eq!(
        store.get(&url(7)).unwrap().unwrap().body.as_deref(),
        Some("Explains the change.\n\n- one detail")
    );
    let body = store.get(&url(8)).unwrap().unwrap().body.unwrap();
    let kept = body.strip_suffix(BODY_TRUNCATED).unwrap();
    assert_eq!(
        kept,
        "a".repeat(MAX_BODY_BYTES - 1),
        "the family is never split"
    );

    // A failed check keeps the stored description.
    store.make_due(T + 1).unwrap();
    let gh = Script::new([Ok(output(1, "", "gh: authentication required"))]);
    store.refresh(&gh, T + 1, None).unwrap();
    assert_eq!(
        store.get(&url(7)).unwrap().unwrap().body.as_deref(),
        Some("Explains the change.\n\n- one detail")
    );
}

#[test]
fn next_due_is_the_earliest_scheduled_check() {
    let (_scratch, mut store) = open_store();
    assert_eq!(store.next_due().unwrap(), None);
    add(&mut store, 7, T);
    add(&mut store, 8, T + 5);
    assert_eq!(store.next_due().unwrap(), Some(T));
    let gh = Script::new([Ok(reply(&[
        pull_request("MERGED", false, None, None),
        pull_request("OPEN", false, None, Some("PENDING")),
    ]))]);
    store.refresh(&gh, T + 5, None).unwrap();
    assert_eq!(store.next_due().unwrap(), Some(T + 5 + 60));
}

fn status(ci: Ci) -> Status {
    Status {
        title: "Improve the example".into(),
        state: State::Open,
        review: Review::None,
        ci,
        head_ref: "feature/example".into(),
        body: String::new(),
    }
}

fn record(store: &mut Store, claimed: &[Claimed], ci: Ci, now: i64) -> Refresh {
    let mut outcome = Refresh::default();
    let results = claimed.iter().map(|_| Ok(status(ci))).collect();
    store.record(claimed, results, now, &mut outcome).unwrap();
    outcome
}

#[test]
fn add_and_make_due_never_steal_an_active_claim() {
    for make_due in [false, true] {
        let scratch = Scratch::new();
        let path = scratch.store();
        let mut first = Store::open(&path).unwrap();
        let mut second = Store::open(&path).unwrap();
        add(&mut first, 7, T);
        let claimed = first.claim(T, None).unwrap();
        assert_eq!(claimed.len(), 1);

        if make_due {
            assert_eq!(second.make_due(T + 1).unwrap(), 1);
        } else {
            add(&mut second, 7, T + 1);
        }
        assert!(second.claim(T + 1, None).unwrap().is_empty(), "{make_due}");
        assert_eq!(
            second.list(T + 1).unwrap().due,
            0,
            "a leased row is not due"
        );
        assert_eq!(second.next_due().unwrap(), Some(T + LEASE_SECS));

        let outcome = record(&mut first, &claimed, Ci::Success, T + 2);
        assert_eq!(outcome.refreshed, 1);
        let entry = second.get(&url(7)).unwrap().unwrap();
        assert_eq!(
            (entry.ci, entry.checked_at, entry.next_check_at),
            (Ci::Success, Some(T + 2), Some(T + 2 + 600))
        );
        assert_eq!(second.next_due().unwrap(), Some(T + 2 + 600));
    }
}

#[test]
fn an_expired_lease_is_reclaimable() {
    let scratch = Scratch::new();
    let path = scratch.store();
    let mut first = Store::open(&path).unwrap();
    let mut second = Store::open(&path).unwrap();
    add(&mut first, 7, T);
    assert_eq!(first.claim(T, None).unwrap().len(), 1);
    assert!(second.claim(T + LEASE_SECS - 1, None).unwrap().is_empty());
    let reclaimed = second.claim(T + LEASE_SECS, None).unwrap();
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(
        record(&mut second, &reclaimed, Ci::Pending, T + LEASE_SECS).refreshed,
        1
    );
}

#[test]
fn results_of_a_superseded_claim_are_dropped() {
    let scratch = Scratch::new();
    let path = scratch.store();
    let mut first = Store::open(&path).unwrap();
    let mut second = Store::open(&path).unwrap();
    add(&mut first, 7, T);
    let stale = first.claim(T, None).unwrap();
    let newer = second.claim(T + LEASE_SECS, None).unwrap();
    assert_eq!(
        record(&mut second, &newer, Ci::Success, T + LEASE_SECS).refreshed,
        1
    );

    // The older response arrives last and must not overwrite the newer one.
    assert_eq!(
        record(&mut first, &stale, Ci::Pending, T + LEASE_SECS + 1).refreshed,
        0
    );
    let mut outcome = Refresh::default();
    first
        .record(
            &stale,
            vec![Err("gh: timed out".into())],
            T + LEASE_SECS + 1,
            &mut outcome,
        )
        .unwrap();
    assert_eq!((outcome.failed, outcome.failures.len()), (0, 0));
    let entry = first.get(&url(7)).unwrap().unwrap();
    assert_eq!(
        (entry.ci, entry.error, entry.checked_at, entry.next_check_at),
        (
            Ci::Success,
            None,
            Some(T + LEASE_SECS),
            Some(T + LEASE_SECS + 600)
        )
    );

    // Nor may it land on a row removed and added again while it was in flight.
    let stale = first.claim(T + LEASE_SECS + 600, None).unwrap();
    first.remove(&url(7)).unwrap();
    add(&mut second, 7, T + LEASE_SECS + 601);
    let newer = second.claim(T + LEASE_SECS + 601, None).unwrap();
    assert_eq!(
        record(&mut first, &stale, Ci::Failure, T + LEASE_SECS + 602).refreshed,
        0
    );
    assert_eq!(
        second.get(&url(7)).unwrap().unwrap().checked_at,
        None,
        "the stale result was dropped"
    );
    assert_eq!(
        record(&mut second, &newer, Ci::Success, T + LEASE_SECS + 603).refreshed,
        1
    );
}

#[test]
fn a_rex_session_is_recorded_and_validated() {
    let (_scratch, mut store) = open_store();
    let added = store
        .add(
            &NewEntry {
                url: &url(7),
                added_by: None,
                checkout: None,
                rex_session: Some("session:work"),
            },
            T,
        )
        .unwrap();
    assert_eq!(added.rex_session.as_deref(), Some("session:work"));
    let invalid = store.add(
        &NewEntry {
            url: &url(8),
            added_by: None,
            checkout: None,
            rex_session: Some("session:\nwork"),
        },
        T,
    );
    assert!(
        matches!(invalid, Err(Error::Invalid("Rex session"))),
        "{invalid:?}"
    );
}
