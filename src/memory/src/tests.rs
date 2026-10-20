use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    store: Store,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dots-memory-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Self {
            store: Store::new(root).unwrap(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.store.root);
    }
}

#[test]
fn revision_distinguishes_absent_empty_and_different_bytes() {
    assert_eq!(revision(None), "missing");
    assert_ne!(revision(None), revision(Some("")));
    assert_ne!(revision(Some("a")), revision(Some("a\n")));
    assert_eq!(revision(Some("é")), revision(Some("é")));
}

#[test]
fn excerpts_preserve_graphemes_and_byte_budget() {
    for text in ["cafe\u{301}", "a👨‍👩‍👧b", "é界"] {
        for limit in 0..=text.len() {
            let excerpt = bounded(text, limit);
            assert!(excerpt.len() <= limit);
            assert!(text.starts_with(&excerpt));
            assert!(
                excerpt.is_empty()
                    || text
                        .grapheme_indices(true)
                        .any(|(start, cluster)| start + cluster.len() == excerpt.len())
            );
        }
    }
}

#[test]
fn project_keys_preserve_existing_normalization() {
    assert_eq!(
        project_key("/Users/example/dev/repo"),
        "Users-example-dev-repo"
    );
    assert_eq!(project_key("/a/é界/b"), "a-b");
    assert_eq!(
        project_key("/a/path with spaces/repo.name_1-2"),
        "a-path-with-spaces-repo.name_1-2"
    );
}

#[test]
fn lock_failure_is_recoverable_without_deleting_another_writers_lock() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    fs::create_dir_all(&store.root).unwrap();
    let lock = Lock::acquire(store.root.join(".write-lock")).unwrap();
    assert!(
        store
            .publish(Path::new("MEMORY.md"), "missing", Some("first"))
            .is_err()
    );
    assert!(lock.path.join("owner").exists());
    drop(lock);
    let first = store
        .publish(Path::new("MEMORY.md"), "missing", Some("first"))
        .unwrap();
    assert!(
        store
            .publish(Path::new("MEMORY.md"), "missing", Some("second"))
            .is_err()
    );
    assert_eq!(
        store.read(Path::new("MEMORY.md"), 1, 100).unwrap().revision,
        first.revision
    );
    assert!(!store.root.join(".write-lock").exists());
}

#[test]
fn writes_are_complete_and_reads_have_explicit_limits() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    let path = Path::new("wiki/topic.md");
    let first = store.publish(path, "missing", Some("a\nb\nc")).unwrap();
    let page = store.read(path, 2, 1).unwrap();
    assert_eq!(page.content, "b\n");
    assert_eq!(page.next_line, Some(3));
    assert_eq!(page.revision, first.revision);
    assert!(store.read(path, 0, 1).is_err());
    assert!(store.read(path, 1, 201).is_err());
    assert!(
        store
            .publish(path, &first.revision, Some(&"x".repeat(MAX_FILE_BYTES + 1)))
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(store.root.join(path)).unwrap(),
        "a\nb\nc"
    );
    assert!(
        store
            .publish(Path::new("../escape.md"), "missing", Some("bad"))
            .is_err()
    );
    store.publish(path, &first.revision, None).unwrap();
    assert!(!store.read(path, 1, 1).unwrap().exists);
}
