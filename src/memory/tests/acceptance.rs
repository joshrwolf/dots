//! Process-death acceptance with a re-executed test binary and isolated files.
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use dots_memory::Store;
use dots_memory::mutation_lock::MutationLock;

#[test]
fn lock_holder_fixture() {
    let Some(root) = std::env::var_os("DOTS_MEMORY_LOCK_FIXTURE") else {
        return;
    };
    let root = Path::new(&root);
    let _lock = MutationLock::acquire(&root.join("mutation.lock")).unwrap();
    fs::write(root.join(".staged-interrupted"), "partial draft").unwrap();
    fs::write(root.join("ready"), "locked").unwrap();
    // The parent kills this process through the bounded shared runner.
    std::thread::sleep(Duration::from_secs(60));
    panic!("fixture was not killed");
}

#[test]
fn killed_writer_releases_lock_without_removing_interrupted_bytes() {
    let root = std::env::temp_dir().join(format!(
        "dots-memory-crash-{}-{:016x}",
        std::process::id(),
        getrandom::u64().unwrap()
    ));
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "lock_holder_fixture", "--nocapture"])
        .env("DOTS_MEMORY_LOCK_FIXTURE", &root);
    let result = dots_process::output(command, Duration::from_secs(2), 8192);
    assert!(
        matches!(result, Err(dots_process::Error::Timeout(_))),
        "{result:?}"
    );
    assert_eq!(fs::read_to_string(root.join("ready")).unwrap(), "locked");
    let store = Store::try_new(root.clone(), root.join("mutation.lock")).unwrap();
    store
        .publish(
            Path::new("wiki/nested/topic.md"),
            "missing",
            Some("complete"),
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".staged-interrupted")).unwrap(),
        "partial draft"
    );
    assert_eq!(
        fs::read_to_string(root.join("wiki/nested/topic.md")).unwrap(),
        "complete"
    );
    assert!(root.join("mutation.lock").is_file());
    fs::remove_dir_all(root).unwrap();
}
