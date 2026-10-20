//! A disposable Git checkout with one committed and one working-tree change.
use std::fs;
use std::os::unix::fs::DirBuilderExt as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) struct Checkout(pub(crate) PathBuf);
impl Checkout {
    pub(crate) fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("review-editor-{}-{nonce}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let fixture = Self(fs::canonicalize(root).unwrap());
        fixture.git(&["init", "--quiet"]);
        fs::write(fixture.0.join("example.txt"), "original\n").unwrap();
        fixture.git(&["add", "example.txt"]);
        fixture.git(&["commit", "--quiet", "-m", "Initial fixture"]);
        fs::write(fixture.0.join("example.txt"), "changed\n").unwrap();
        fixture
    }
    pub(crate) fn git(&self, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.0)
            .args([
                "-c",
                "user.name=Example",
                "-c",
                "user.email=example@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args);
        let output = dots_process::output(command, Duration::from_secs(10), 64 * 1024).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().into()
    }
}
impl Drop for Checkout {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
