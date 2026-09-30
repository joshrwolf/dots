use super::*;
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    os::unix::process::ExitStatusExt as _,
    process::{ExitStatus, Output},
};
const URL: &str = "https://github.com/example/project/pull/7";
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct Fake {
    replies: RefCell<VecDeque<Value>>,
    calls: RefCell<Vec<Vec<String>>>,
    saved: RefCell<HashMap<String, Vec<u8>>>,
}
impl Fake {
    fn new(replies: Vec<Value>) -> Self {
        Self {
            replies: RefCell::new(replies.into()),
            ..Self::default()
        }
    }
}
impl Provider for Fake {
    fn gh(&self, args: &[&str]) -> Result<Output> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(ToString::to_string).collect());
        let reply = self
            .replies
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| Error::InvalidResponse("unexpected network query".into()))?;
        Ok(Output {
            status: ExitStatus::from_raw(0),
            stdout: serde_json::to_vec(&reply)?,
            stderr: vec![],
        })
    }
    fn local_head(&self) -> Result<Option<String>> {
        Err(target::invalid())
    }
    fn save_snapshot(&self, reference: &SnapshotRef, bytes: &[u8], _: u64) -> bool {
        self.saved
            .borrow_mut()
            .insert(reference.to_string(), bytes.to_vec());
        true
    }
    fn saved_snapshot(&self, reference: &SnapshotRef) -> Option<crate::CachedLog> {
        Some(crate::CachedLog {
            bytes: self.saved.borrow().get(&reference.to_string())?.clone(),
            fetched_at: crate::cache::now(),
        })
    }
}
fn comment(id: &str) -> Value {
    json!({"id":id,"databaseId":42,"author":null,"body":"Please check cafe\u{301} 👩‍💻","url":format!("{URL}#discussion_r42"),
        "createdAt":"2026-01-01T00:00:00Z","replyTo":null})
}
fn item(id: &str, resolved: bool, outdated: bool) -> Value {
    json!({"id":id,"isResolved":resolved,"isOutdated":outdated,"path":"src/example.rs",
        "line":null,"startLine":null,"originalLine":7,"originalStartLine":5,"diffSide":"RIGHT","startDiffSide":"RIGHT","subjectType":"LINE",
        "comments":{"totalCount":2,"nodes":[comment("COMMENT_1")]}})
}
fn index(nodes: &[Value], total: usize, cursor: Option<&str>) -> Value {
    json!({"data":{"repository":{"pullRequest":{"url":URL,"headRefOid":HEAD,"reviewThreads":{
        "totalCount":total,"nodes":nodes,"pageInfo":{"hasNextPage":cursor.is_some(),"endCursor":cursor}}}}}})
}
fn conversation(ids: &[&str], total: usize, cursor: Option<&str>) -> Value {
    let mut value = item("THREAD_1", false, true);
    value
        .as_object_mut()
        .unwrap()
        .insert("pullRequest".into(), json!({"url":URL,"headRefOid":HEAD}));
    value.as_object_mut().unwrap().insert(
        "anchor".into(),
        json!({"nodes":[{"url":"https://github.com/example/project/pull/7#discussion_r42","databaseId":42,"diffHunk":"@@ -5 +5 @@
-old
+new","originalCommit":{"oid":HEAD},"commit":{"oid":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}]}),
    );
    value.as_object_mut().unwrap().insert(
        "comments".into(),
        json!({"totalCount":total,"nodes":ids.iter().map(|id|comment(id)).collect::<Vec<_>>(),
        "pageInfo":{"hasNextPage":cursor.is_some(),"endCursor":cursor}}),
    );
    json!({"data":{"node":value}})
}
fn options() -> ListOptions<'static> {
    ListOptions {
        pr: Some(URL),
        repository: None,
        snapshot: None,
        all: false,
        offset: 0,
        limit: 1,
    }
}
fn reference() -> ThreadRef {
    format!("thread:github.com/example/project:7:{HEAD}:THREAD_1")
        .parse()
        .unwrap()
}
fn thread_options() -> ThreadOptions<'static> {
    ThreadOptions {
        snapshot: None,
        offset: 0,
        limit: 1,
    }
}

#[test]
fn paginated_index_preserves_unresolved_outdated_threads_and_snapshots() {
    let fake = Fake::new(vec![
        index(&[item("THREAD_1", false, true)], 3, Some("CURSOR")),
        index(
            &[item("THREAD_2", true, true), item("THREAD_3", false, false)],
            3,
            None,
        ),
    ]);
    let first = list(&fake, options()).unwrap();
    assert_eq!(first.summary.total, 3);
    assert_eq!(first.summary.unresolved, 2);
    assert_eq!(first.summary.unresolved_outdated, 1);
    assert_eq!(first.returned, 1);
    assert!(!first.complete);
    assert!(first.threads.first().unwrap().outdated);
    assert!(!first.threads.first().unwrap().conversation_complete);
    assert!(
        first
            .threads
            .first()
            .unwrap()
            .preview
            .as_ref()
            .unwrap()
            .author
            .is_none()
    );
    assert!(
        fake.calls
            .borrow()
            .get(1)
            .unwrap()
            .iter()
            .any(|arg| arg == "cursor=CURSOR")
    );
    let second = list(
        &fake,
        ListOptions {
            pr: None,
            snapshot: first.snapshot.as_ref(),
            offset: 1,
            ..options()
        },
    )
    .unwrap();
    assert_eq!(second.threads.first().unwrap().reference.node, "THREAD_3");
    assert!(!second.has_more);
    assert!(second.next_command.is_none());
    assert_eq!(second.observed_at, first.observed_at);
    assert_eq!(
        second.threads.first().unwrap().location.original_start_line,
        Some(5)
    );
    assert_eq!(fake.calls.borrow().len(), 2);
}
#[test]
fn exact_conversation_pages_reuse_snapshot_and_preserve_context() {
    let fake = Fake::new(vec![
        conversation(&["COMMENT_1"], 2, Some("NEXT")),
        conversation(&["COMMENT_2"], 2, None),
    ]);
    let first = thread(&fake, &reference(), thread_options()).unwrap();
    assert!(first.has_more);
    let context = serde_json::to_value(first.context.as_ref().unwrap()).unwrap();
    assert_eq!(context.get("original_commit_id").unwrap(), HEAD);
    assert_eq!(
        context.get("commit_id").unwrap(),
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    );
    assert!(context.get("reviewed_commit").is_none());
    assert!(first.refresh_command.ends_with("--limit 1"));
    assert_eq!(first.comments_total, 2);
    assert_eq!(first.returned, 1);
    assert!(!first.complete);
    assert!(first.next_command.as_ref().unwrap().contains("--snapshot"));
    let second = thread(
        &fake,
        &reference(),
        ThreadOptions {
            snapshot: first.snapshot.as_ref(),
            offset: 1,
            ..thread_options()
        },
    )
    .unwrap();
    assert!(second.context.is_none());
    assert!(
        second
            .context_command
            .as_ref()
            .unwrap()
            .contains("--snapshot")
    );
    assert!(
        second
            .context_command
            .as_ref()
            .unwrap()
            .ends_with("--offset 0")
    );
    assert_eq!(second.comments.first().unwrap().id, "COMMENT_2");
    assert!(matches!(second.source, ObservationSource::Snapshot));
    assert_eq!(
        first
            .context
            .as_ref()
            .unwrap()
            .original_commit_id
            .as_deref(),
        Some(HEAD)
    );
    assert!(!second.has_more);
    assert!(second.next_command.is_none());
    assert_eq!(second.observed_at, first.observed_at);
    assert_eq!(fake.calls.borrow().len(), 2);
    let other: ThreadRef = format!("thread:github.com/example/project:7:{HEAD}:OTHER")
        .parse()
        .unwrap();
    assert!(
        thread(
            &fake,
            &other,
            ThreadOptions {
                snapshot: first.snapshot.as_ref(),
                ..thread_options()
            }
        )
        .is_err()
    );
    assert_eq!(fake.calls.borrow().len(), 2);
}
#[test]
fn incomplete_duplicate_changing_and_error_responses_fail_closed() {
    let duplicate = vec![
        index(&[item("THREAD_1", false, false)], 2, Some("NEXT")),
        index(&[item("THREAD_1", false, false)], 2, None),
    ];
    assert!(list(&Fake::new(duplicate), options()).is_err());
    assert!(list(&Fake::new(vec![index(&[], 1, None)]), options()).is_err());
    let mut changed_head = index(&[item("THREAD_2", false, false)], 2, None);
    *changed_head
        .pointer_mut("/data/repository/pullRequest/headRefOid")
        .unwrap() = json!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    assert!(
        list(
            &Fake::new(vec![
                index(&[item("THREAD_1", false, false)], 2, Some("NEXT")),
                changed_head
            ]),
            options()
        )
        .is_err()
    );
    assert!(
        list(
            &Fake::new(vec![json!({"data":null,"errors":[{"message":"denied"}]})]),
            options()
        )
        .is_err()
    );
    let mut wrong = conversation(&["COMMENT_1"], 1, None);
    *wrong.pointer_mut("/data/node/pullRequest/url").unwrap() =
        json!("https://github.com/other/project/pull/7");
    assert!(thread(&Fake::new(vec![wrong]), &reference(), thread_options()).is_err());
    assert!(
        thread(
            &Fake::new(vec![conversation(&["COMMENT_1"], 2, None)]),
            &reference(),
            thread_options()
        )
        .is_err()
    );
}
#[test]
fn empty_results_are_distinct_from_missing_repository_and_input_errors() {
    let fake = Fake::new(vec![index(&[], 0, None)]);
    let empty = list(&fake, options()).unwrap();
    assert!(empty.complete);
    assert_eq!(empty.summary.total, 0);
    assert!(
        list(
            &Fake::new(vec![json!({"data":{"repository":null}})]),
            options()
        )
        .is_err()
    );
    let repo = "other/project".parse().unwrap();
    assert!(
        list(
            &Fake::default(),
            ListOptions {
                repository: Some(&repo),
                ..options()
            }
        )
        .is_err()
    );
    assert!(
        list(
            &Fake::default(),
            ListOptions {
                limit: 0,
                ..options()
            }
        )
        .is_err()
    );
    for value in [
        "thread:github.com/example/project:7:bad:ID",
        "thread:github.com/example/project:7:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:$(id)",
    ] {
        assert!(value.parse::<ThreadRef>().is_err());
    }
}
#[test]
fn bodies_are_grapheme_bounded_and_snapshot_failures_never_query() {
    let mut reply = conversation(&["COMMENT_1"], 1, None);
    *reply
        .pointer_mut("/data/node/comments/nodes/0/body")
        .unwrap() = json!("👩‍💻".repeat(1000));
    let fake = Fake::new(vec![reply]);
    let result = thread(&fake, &reference(), thread_options()).unwrap();
    let body = &result.comments.first().unwrap().body;
    assert!(body.truncated);
    assert!(body.text.len() <= 4096);
    assert!(body.text.graphemes(true).all(|g| g == "👩‍💻"));
    let snapshot = result.snapshot.unwrap();
    fake.saved
        .borrow_mut()
        .get_mut(&snapshot.to_string())
        .unwrap()
        .push(b'x');
    assert!(
        thread(
            &fake,
            &reference(),
            ThreadOptions {
                snapshot: Some(&snapshot),
                ..thread_options()
            }
        )
        .is_err()
    );
    fake.saved.borrow_mut().clear();
    assert!(
        thread(
            &fake,
            &reference(),
            ThreadOptions {
                snapshot: Some(&snapshot),
                ..thread_options()
            }
        )
        .is_err()
    );
    assert_eq!(fake.calls.borrow().len(), 1);
}

#[test]
fn expired_observations_and_conversation_budgets_fail_explicitly() {
    let fake = Fake::new(vec![conversation(&["C1"], 1, None)]);
    let saved = collect_thread(&fake, &reference()).unwrap();
    let snapshot = save(
        &fake,
        &reference().url(),
        &saved,
        crate::cache::now().saturating_sub(300),
    )
    .unwrap();
    assert!(
        thread(
            &fake,
            &reference(),
            ThreadOptions {
                snapshot: Some(&snapshot),
                ..thread_options()
            }
        )
        .is_err()
    );
    assert_eq!(fake.calls.borrow().len(), 1);
    let replies = (0..8)
        .map(|n| conversation(&[&format!("C{n}")], 201, Some(&format!("PAGE{n}"))))
        .collect();
    let fake = Fake::new(replies);
    let error = thread(&fake, &reference(), thread_options()).unwrap_err();
    assert!(error.diagnostic().contains("200-comment query budget"));
    assert_eq!(fake.calls.borrow().len(), 8);
}

#[test]
fn refresh_preserves_selection_and_page_size_but_resets_offset() {
    let fake = Fake::new(vec![index(&[item("THREAD_1", true, true)], 1, None)]);
    let first = list(
        &fake,
        ListOptions {
            all: true,
            limit: 2,
            ..options()
        },
    )
    .unwrap();
    let expected = format!("dots github reviews --pr {URL} --all --limit 2");
    assert_eq!(first.refresh_command, expected);
    let saved = list(
        &fake,
        ListOptions {
            pr: None,
            snapshot: first.snapshot.as_ref(),
            all: true,
            limit: 2,
            offset: 1,
            ..options()
        },
    )
    .unwrap();
    assert_eq!(saved.refresh_command, expected);
    assert!(!saved.has_more);
    assert_eq!(fake.calls.borrow().len(), 1);
}
