use super as memory;
use super::mutation_lock::MutationLock;
use super::{MAX_FILE_BYTES, Store};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use unicode_segmentation::UnicodeSegmentation as _;

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
            store: Store::try_new(root.clone(), root.join("mutation.lock")).unwrap(),
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
    assert_eq!(memory::revision(None), "missing");
    assert_ne!(memory::revision(None), memory::revision(Some("")));
    assert_ne!(memory::revision(Some("a")), memory::revision(Some("a\n")));
    assert_eq!(memory::revision(Some("é")), memory::revision(Some("é")));
}

#[test]
fn missing_and_present_empty_documents_have_distinct_revisions() {
    let fixture = Fixture::new();
    let path = Path::new("wiki/empty.md");
    let missing = fixture.store.read(path, 1, 1).unwrap();
    assert!(!missing.exists);
    assert_eq!(missing.revision, "missing");
    assert_eq!(missing.content, "");

    let published = fixture.store.publish(path, "missing", Some("")).unwrap();
    let empty = fixture.store.read(path, 1, 1).unwrap();
    assert!(empty.exists);
    assert_eq!(empty.revision, published.revision);
    assert_ne!(empty.revision, missing.revision);
    assert_eq!(empty.content, "");
    assert_eq!(empty.next_line, None);
}

#[test]
fn excerpts_preserve_graphemes_and_byte_budget() {
    for text in ["cafe\u{301}", "a👨\u{200D}👩\u{200D}👧b", "é界"] {
        for limit in 0..=text.len() {
            let excerpt = memory::bounded(text, limit);
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
fn lock_failure_is_recoverable_without_deleting_another_writers_lock() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    fs::create_dir_all(&store.root).unwrap();
    let lock = MutationLock::acquire(&store.lock_path).unwrap();
    assert!(
        store
            .publish(Path::new("MEMORY.md"), "missing", Some("first"))
            .is_err()
    );
    assert!(store.lock_path.is_file());
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
    assert!(store.lock_path.is_file());
    assert!(MutationLock::acquire(&store.lock_path).is_ok());
}

#[test]
fn one_lock_coordinates_distinct_document_scopes() {
    let fixture = Fixture::new();
    let first = &fixture.store;
    let second = Store::try_new(first.root.join("other"), first.lock_path.clone()).unwrap();
    let lock = MutationLock::acquire(&first.lock_path).unwrap();
    for store in [first, &second] {
        let error = store.publish(Path::new("MEMORY.md"), "missing", Some("a"));
        assert!(error.unwrap_err().to_string().contains("busy"));
        assert!(!store.read(Path::new("MEMORY.md"), 1, 1).unwrap().exists);
    }
    drop(lock);
    for store in [first, &second] {
        store
            .publish(Path::new("MEMORY.md"), "missing", Some("a"))
            .unwrap();
    }
    assert!(
        first.lock_path.is_file(),
        "the lock inode must remain stable"
    );
}

#[test]
fn reads_do_not_create_a_lock_or_scope() {
    let fixture = Fixture::new();
    assert!(
        !fixture
            .store
            .read(Path::new("MEMORY.md"), 1, 1)
            .unwrap()
            .exists
    );
    assert!(!fixture.store.root.exists());
}

#[cfg(unix)]
#[test]
fn lock_symlinks_are_refused_without_touching_the_target() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.store.root).unwrap();
    let target = fixture.store.root.join("unrelated");
    fs::write(&target, "unchanged").unwrap();
    std::os::unix::fs::symlink(&target, &fixture.store.lock_path).unwrap();
    assert!(MutationLock::acquire(&fixture.store.lock_path).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "unchanged");
}

#[test]
fn interrupted_staging_files_remain_untouched_by_publication() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.store.root).unwrap();
    for nonce in 0..128 {
        fs::write(
            fixture.store.root.join(format!(".staged-{nonce:032x}")),
            "interrupted draft",
        )
        .unwrap();
    }
    fixture
        .store
        .publish(Path::new("MEMORY.md"), "missing", Some("published"))
        .unwrap();
    assert_eq!(
        fs::read_to_string(fixture.store.root.join("MEMORY.md")).unwrap(),
        "published"
    );
    for nonce in 0..128 {
        assert_eq!(
            fs::read_to_string(fixture.store.root.join(format!(".staged-{nonce:032x}"))).unwrap(),
            "interrupted draft"
        );
    }
    assert_eq!(fs::read_dir(&fixture.store.root).unwrap().count(), 130);
}

#[cfg(unix)]
#[test]
fn publication_does_not_open_unreadable_external_ancestors() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new();
    let ancestor = fixture.store.root.join("external");
    let storage = ancestor.join("storage");
    fs::create_dir_all(&storage).unwrap();
    let store = Store::try_new(storage.join("knowledge"), storage.join("mutation.lock")).unwrap();
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o311)).unwrap();
    let result = store.publish(Path::new("wiki/topic.md"), "missing", Some("content"));
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o700)).unwrap();
    result.unwrap();
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
