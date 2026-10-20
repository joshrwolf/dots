//! The Lua behavior suite in an isolated headless Neovim.
#![cfg(test)]
mod support;

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use support::Checkout;

#[test]
fn lua_behavior_suite() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tests = manifest.join("tests");
    let lock: serde_json::Value =
        serde_json::from_slice(&fs::read(tests.join("nvim/nvim-pack-lock.json")).unwrap()).unwrap();
    let revision = lock
        .pointer("/plugins/mini.test/rev")
        .and_then(serde_json::Value::as_str)
        .unwrap();
    let dependency = manifest.join("../target/test-deps/nvim/site/pack/core/opt/mini.test");
    assert!(
        dependency.join("lua/mini/test.lua").is_file(),
        "run mise run //src/herdr/review:deps first"
    );
    for (args, expected) in [
        (vec!["rev-parse", "HEAD"], revision),
        (vec!["status", "--porcelain", "--untracked-files=all"], ""),
    ] {
        let mut command = Command::new("git");
        command.current_dir(&dependency).args(args);
        let output = dots_process::output(command, Duration::from_secs(10), 16 * 1024).unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let actual = String::from_utf8(output.stdout).unwrap();
        // vim.pack generates the help index; it is not dependency source.
        let actual = actual
            .lines()
            .filter(|line| *line != "?? doc/tags")
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(actual.trim(), expected, "mini.test checkout changed");
    }
    let scratch = Checkout::new();
    let mut command = Command::new("nvim");
    command
        .args([
            "--headless",
            "-u",
            "NONE",
            "-i",
            "NONE",
            "-c",
            "lua dofile(vim.env.HERDR_REVIEW_TESTS .. '/lua/runner.lua')",
        ])
        .env("HERDR_REVIEW_TESTS", &tests)
        .env(
            "HERDR_REVIEW_CONFIG",
            manifest.join("../../../home/nvim/.config/nvim"),
        )
        .env("HERDR_REVIEW_SCRATCH", &scratch.0)
        .env("MINI_TEST_PATH", dependency)
        .env("NVIM_APPNAME", "nvim")
        .env("XDG_CONFIG_HOME", scratch.0.join("config"))
        .env("XDG_DATA_HOME", scratch.0.join("data"))
        .env("XDG_STATE_HOME", scratch.0.join("state"))
        .env("XDG_CACHE_HOME", scratch.0.join("cache"))
        .env("NVIM_LOG_FILE", scratch.0.join("nvim.log"));
    let output = dots_process::output(command, Duration::from_secs(90), 1024 * 1024).unwrap();
    assert!(
        output.status.success(),
        "mini.test failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
