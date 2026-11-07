use dots_documents::staged_directory::StagedDirectory;

use super::*;

#[test]
fn a_newer_schema_is_refused_and_never_rewritten() -> Result<()> {
    let scratch = StagedDirectory::create(&std::env::temp_dir())?;
    let root = scratch.path().join("state");
    let open = Database::open(&root)?;
    open.check()?;
    let newer =
        Connection::open(root.join(DATABASE)).map_err(|source| database(&root, "test", source))?;
    newer
        .pragma_update(None, "user_version", SCHEMA + 1)
        .map_err(|source| database(&root, "test", source))?;
    assert!(matches!(
        open.check(),
        Err(Error::Schema { found, supported: SCHEMA }) if found == SCHEMA + 1
    ));
    assert!(matches!(Database::open(&root), Err(Error::Schema { .. })));
    assert_eq!(schema(&newer, &root)?, SCHEMA + 1);
    Ok(())
}

#[test]
#[expect(
    clippy::redundant_closure_for_method_calls,
    reason = "the method path fixes the state's lifetime"
)]
fn the_machine_identity_is_allocated_once() -> Result<()> {
    let scratch = StagedDirectory::create(&std::env::temp_dir())?;
    let root = scratch.path().join("state");
    let first = Database::open(&root)?.modify(|state| state.machine())?;
    let second = Database::open(&root)?.modify(|state| state.machine())?;
    assert_eq!(first, second);
    Ok(())
}
