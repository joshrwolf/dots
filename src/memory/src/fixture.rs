use crate::{Result, Store};
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct Fixture {
    pub(crate) parent: PathBuf,
    pub(crate) store: Store,
}
impl Fixture {
    pub(crate) fn new() -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt as _;
        let parent = std::env::temp_dir().join(format!(
            "dots-knowledge-{}-{:016x}",
            std::process::id(),
            getrandom::u64()?
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&parent)
            .map_err(|error| crate::io("create test home", &parent, error))?;
        let store = Store::initialize(&parent.join("knowledge"), "neutral-host")?;
        Ok(Self { parent, store })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.parent);
    }
}
