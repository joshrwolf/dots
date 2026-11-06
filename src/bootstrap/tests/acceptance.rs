#![cfg(test)]

use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);
type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    source: PathBuf,
    stage: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "dots-native-bootstrap-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let f = Self {
            home: root.join("home"),
            source: root.join("public source"),
            stage: root.join("transfer stage"),
            root,
        };
        fs::create_dir(&f.home)?;
        fs::create_dir(&f.source)?;
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        f.copy_tree(
            &repo.join("rust-toolchain.toml"),
            &f.source.join("rust-toolchain.toml"),
        )?;
        f.copy_tree(&repo.join("bootstrap"), &f.source.join("bootstrap"))?;
        let antidote = f.root.join("antidote-source");
        fs::create_dir(&antidote)?;
        f.write(
            &antidote.join("fixture"),
            "Neutral source-only dependency.\n",
        )?;
        f.init_repo(&antidote)?;
        let machine = f.source.join("bootstrap/mise.toml");
        let content = fs::read_to_string(&machine)?;
        f.write(
            &machine,
            &content.replace(
                "https://github.com/mattmc3/antidote.git",
                &antidote.display().to_string(),
            ),
        )?;
        // File/source acceptance does not resolve or install the machine toolset.
        f.copy_tree(
            &repo.join("src/codediff.nvim"),
            &f.source.join("src/codediff.nvim"),
        )?;
        f.write(&f.source.join("bootstrap/tools.toml"), "[tools]\n")?;
        f.write(&f.source.join("src/codediff.nvim/tools.toml"), "[tools]\n")?;
        f.copy_tree(&repo.join("home"), &f.source.join("home"))?;
        f.copy_tree(&repo.join("package.json"), &f.source.join("package.json"))?;
        f.copy_tree(
            &repo.join("pnpm-workspace.yaml"),
            &f.source.join("pnpm-workspace.yaml"),
        )?;
        f.write(
            &f.source.join("skills/public-example/SKILL.md"),
            "---\nname: public-example\ndescription: Neutral public skill\n---\nPublic skill.\n",
        )?;
        f.write(&f.source.join(".gitignore"), "/private/\n")?;
        f.write(
            &f.source.join("mise.toml"),
            "[env]\nPARENT_DEVELOPMENT_CONFIG = \"must-not-load\"\n",
        )?;
        f.init_repo(&f.source)?;
        f.copy_tree(&f.source.join("bootstrap/source"), &f.stage)?;
        f.stage_inputs(None)?;
        Ok(f)
    }

    fn checkout(&self) -> PathBuf {
        self.home.join(".local/share/dots/checkouts/dots")
    }

    fn write(&self, path: &Path, content: &str) -> Result {
        assert!(
            path.starts_with(&self.root),
            "write outside fixture: {path:?}"
        );
        fs::create_dir_all(path.parent().ok_or("missing file parent")?)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn copy_tree(&self, from: &Path, to: &Path) -> Result {
        assert!(to.starts_with(&self.root), "copy outside fixture: {to:?}");
        let metadata = fs::symlink_metadata(from)?;
        if metadata.is_symlink() {
            fs::create_dir_all(to.parent().ok_or("missing link parent")?)?;
            unix_fs::symlink(fs::read_link(from)?, to)?;
        } else if metadata.is_dir() {
            fs::create_dir_all(to)?;
            for entry in fs::read_dir(from)? {
                let entry = entry?;
                self.copy_tree(&entry.path(), &to.join(entry.file_name()))?;
            }
        } else {
            fs::create_dir_all(to.parent().ok_or("missing copy parent")?)?;
            fs::copy(from, to)?;
        }
        Ok(())
    }

    fn run(&self, cwd: &Path, program: &str, args: &[&str]) -> Result<Output> {
        let executable = if program == "mise" {
            std::env::var_os("DOTS_TEST_MISE").unwrap_or_else(|| program.into())
        } else {
            program.into()
        };
        let mut command = Command::new(&executable);
        command
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").ok_or("PATH is missing")?)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("MISE_SYSTEM_CONFIG_DIR", self.root.join("system"))
            .env("MISE_SYSTEM_BREW_PREFIX", self.root.join("native-prefix"))
            .env("MISE_BREW_CASK_OPT_APPDIR", self.home.join("Applications"))
            .env(
                "MISE_TRUSTED_CONFIG_PATHS",
                std::env::join_paths([
                    cwd.join("mise.toml"),
                    self.stage.clone(),
                    self.checkout().join("bootstrap"),
                    self.checkout().join("private/bootstrap"),
                ])?,
            )
            .env("MISE_YES", "1")
            .env("NO_COLOR", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", "Example")
            .env("GIT_AUTHOR_EMAIL", "example@example.net")
            .env("GIT_COMMITTER_NAME", "Example")
            .env("GIT_COMMITTER_EMAIL", "example@example.net");
        Ok(dots_process::output(
            command,
            Duration::from_secs(90),
            2 * 1024 * 1024,
        )?)
    }

    fn ok(&self, cwd: &Path, program: &str, args: &[&str]) -> Result<String> {
        let output = self.run(cwd, program, args)?;
        assert!(output.status.success(), "{program} {args:?}: {output:?}");
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> Result<String> {
        self.ok(cwd, "git", args)
    }

    fn init_repo(&self, path: &Path) -> Result {
        self.git(path, &["init", "-b", "main"])?;
        self.commit(path, "Authored fixture")
    }

    fn commit(&self, path: &Path, message: &str) -> Result {
        self.git(path, &["add", "."])?;
        self.git(path, &["commit", "-m", message])?;
        Ok(())
    }

    fn private_source(&self) -> Result<PathBuf> {
        let private = self.root.join("private source");
        fs::create_dir(&private)?;
        if let Some(source) = std::env::var_os("DOTS_TEST_PRIVATE_BOOTSTRAP") {
            self.copy_tree(Path::new(&source), &private.join("bootstrap"))?;
        } else {
            self.write(&private.join("bootstrap/mise.private.toml"), "[dotfile_groups.private-skills]\nroot = \"../private/skills\"\ntarget = \"~/.local/share/dots/skills\"\nmanifest = \"git\"\n\n[dotfile_groups.private-rex]\nroot = \"../private/rex\"\ntarget = \"~/.config/rex\"\nmanifest = \"git\"\n")?;
        }
        self.write(
            &private.join("skills/local-example/SKILL.md"),
            "---\nname: local-example\ndescription: Neutral private skill\n---\nPrivate skill.\n",
        )?;
        self.write(
            &private.join("rex/checkouts/example.json"),
            "{\"version\": 1, \"rules\": {\"example-org/*\": \"~/dev/example/{name}\"}}\n",
        )?;
        self.init_repo(&private)?;
        Ok(private)
    }

    fn stage_inputs(&self, private: Option<&Path>) -> Result {
        let mut inputs = format!(
            "[vars]\ndots_public_url = {}\n",
            serde_json::to_string(&self.source)?
        );
        if let Some(private) = private {
            writeln!(
                inputs,
                "dots_private_url = {}",
                serde_json::to_string(private)?
            )?;
        }
        // This suite verifies authored source/file composition. Native part
        // selection excludes runtime provisioning, which has its own acceptance.
        inputs.push_str("\n[tasks.bootstrap]\ndir = \"{{ env.HOME }}\"\nrun = '\"{{ mise_bin }}\" bootstrap --only files,dotfiles'\n");
        self.write(&self.stage.join("mise.local.toml"), &inputs)
    }

    fn bootstrap(&self, before: &[&str], after: &[&str]) -> Result<Output> {
        let mut args = before.to_vec();
        args.extend(["bootstrap", "--only", "files,repos,dotfiles,task"]);
        args.extend_from_slice(after);
        self.run(&self.stage, "mise", &args)
    }

    fn apply(&self, before: &[&str], after: &[&str]) -> Result {
        let output = self.bootstrap(before, after)?;
        assert!(
            output.status.success(),
            "native source bootstrap: {output:?}"
        );
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if std::thread::panicking() {
                eprintln!("fixture cleanup failed: {error}");
            } else {
                panic!("fixture cleanup failed: {error}");
            }
        }
    }
}

#[cfg(target_os = "macos")]
struct AssetServer {
    url: String,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
impl AssetServer {
    fn new(data: Vec<u8>) -> Result<Self> {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}/fixture.zip", listener.local_addr()?);
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done = stop.clone();
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .expect("set fixture read timeout");
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .expect("set fixture write timeout");
                        let mut request = [0; 8192];
                        let bytes = stream.read(&mut request).expect("read fixture request");
                        assert_ne!(bytes, 0, "empty fixture HTTP request");
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            data.len()
                        )
                        .expect("write fixture headers");
                        stream.write_all(&data).expect("write fixture asset");
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept fixture request: {error}"),
                }
            }
        });
        Ok(Self {
            url,
            stop,
            thread: Some(thread),
        })
    }
}

#[cfg(target_os = "macos")]
impl Drop for AssetServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            assert!(
                thread.join().is_ok() || std::thread::panicking(),
                "fixture asset server failed"
            );
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn bootstrap_app_pin_preserves_vendor_updates_on_apply_and_upgrade() -> Result {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new()?;
    let config = f.root.join("app-setup");
    let initial = r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>net.example.fixture</string><key>CFBundleShortVersionString</key><string>1.0.0</string><key>CFBundleVersion</key><string>1</string></dict></plist>"#;
    f.write(
        &config.join(".miserc.toml"),
        "ceiling_paths = [\"{{ config_root | dirname }}\"]\n",
    )?;
    f.write(&config.join("Example.app/Contents/Info.plist"), initial)?;
    f.write(
        &config.join("Example.app/Contents/resource"),
        "initial installer\n",
    )?;
    f.ok(
        &config,
        "/usr/bin/ditto",
        &[
            "--norsrc",
            "-c",
            "-k",
            "--keepParent",
            "Example.app",
            "fixture.zip",
        ],
    )?;
    let digest = f.ok(&config, "/usr/bin/shasum", &["-a", "256", "fixture.zip"])?;
    let digest = digest
        .split_whitespace()
        .next()
        .ok_or("missing fixture checksum")?;
    let server = AssetServer::new(fs::read(config.join("fixture.zip"))?)?;
    f.write(&config.join("mise.toml"), &format!("[bootstrap.packages.\"macos-app:example\"]\nversion = \"1.0.0\"\nurl = {:?}\nsha256 = {:?}\nartifact = \"Example.app\"\n", server.url, digest))?;
    fs::create_dir_all(f.home.join("Applications"))?;
    f.ok(&config, "mise", &["bootstrap", "packages", "apply"])?;
    let installed = f.home.join("Applications/Example.app");
    let inode = fs::metadata(&installed)?.ino();
    let updated = initial
        .replace("1.0.0", "2.0.0")
        .replace("<string>1</string>", "<string>2</string>");
    f.write(&installed.join("Contents/Info.plist"), &updated)?;
    f.write(
        &installed.join("Contents/resource"),
        "vendor updated content\n",
    )?;
    f.ok(&config, "mise", &["bootstrap", "packages", "apply"])?;
    f.ok(&config, "mise", &["bootstrap", "packages", "upgrade"])?;
    assert_eq!(
        fs::metadata(&installed)?.ino(),
        inode,
        "reapply replaced the updated bundle"
    );
    assert_eq!(
        fs::read_to_string(installed.join("Contents/Info.plist"))?,
        updated
    );
    assert_eq!(
        fs::read_to_string(installed.join("Contents/resource"))?,
        "vendor updated content\n"
    );
    Ok(())
}

#[test]
fn public_source_deploys_native_modules_and_shared_skill_discovery() -> Result {
    let f = Fixture::new()?;
    f.write(&f.home.join(".pi/agent/auth.json"), "{\"fixture\":true}\n")?;
    f.write(&f.home.join(".codex/config.toml"), "# host settings\n")?;
    let output = f.bootstrap(&[], &[])?;
    assert!(
        output.status.success(),
        "native source bootstrap: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(&f.git(&f.checkout(), &["rev-parse", "HEAD"])?),
        "deployed revision missing: {output:?}"
    );
    f.apply(&[], &[])?;
    assert!(f.home.join(".config/mise/conf.d/dots").is_symlink());
    for path in [".agents/skills", ".claude/skills"] {
        assert!(
            f.home.join(path).is_symlink(),
            "directory discovery link missing: {path}"
        );
        assert_eq!(
            f.home.join(path).canonicalize()?,
            f.home.join(".local/share/dots/skills")
        );
        assert!(f.home.join(path).join("public-example/SKILL.md").exists());
    }
    for path in [
        ".zshrc",
        ".config/nvim/init.lua",
        ".config/rex/init.lua",
        ".config/worktrunk/config.toml",
    ] {
        assert!(
            f.home.join(path).is_symlink(),
            "configured runtime file missing: {path}"
        );
    }
    assert!(!f.home.join(".config/rex/tests").exists());
    let codediff = f
        .home
        .join(".local/share/nvim/site/pack/dots/start/codediff.nvim");
    assert!(codediff.join("plugin/codediff.lua").exists());
    assert!(codediff.join("lua/codediff/init.lua").exists());
    for path in ["tests", "AGENTS.md", "mise.toml", "tools.toml"] {
        assert!(
            !codediff.join(path).exists(),
            "plugin development file deployed: {path}"
        );
    }
    assert!(!f.home.join(".config/rex/types").exists());
    assert_eq!(
        fs::read_to_string(f.home.join(".pi/agent/auth.json"))?,
        "{\"fixture\":true}\n"
    );
    assert_eq!(
        fs::read_to_string(f.home.join(".codex/config.toml"))?,
        "# host settings\n"
    );
    assert_eq!(f.git(&f.checkout(), &["status", "--porcelain"])?, "");
    let env = f.ok(&f.home, "mise", &["env", "--json"])?;
    assert!(
        !env.contains("PARENT_DEVELOPMENT_CONFIG"),
        "development config leaked: {env}"
    );
    Ok(())
}

#[test]
fn pi_preferences_merge_without_replacing_machine_state() -> Result {
    use std::os::unix::fs::PermissionsExt as _;

    let f = Fixture::new()?;
    let settings = f.home.join(".pi/agent/settings.json");
    let auth = f.home.join(".pi/agent/auth.json");
    let expected: Value =
        serde_json::from_slice(&fs::read(f.source.join("home/pi/.pi/agent/settings.json"))?)?;
    assert!(
        expected["enabledModels"]
            .as_array()
            .is_some_and(|models| !models.is_empty()),
        "portable preferences need a model shortlist"
    );
    let local = serde_json::json!({
        "deviceId": "neutral-device",
        "packages": ["./local-runtime"],
        "defaultProvider": "example-provider",
        "defaultModel": "host-model",
        "lastChangelogVersion": "0.0.0",
        "theme": "host-theme",
        "compaction": {"reserveTokens": 16384},
        "unowned": {"keep": true}
    });
    f.write(&settings, &serde_json::to_string_pretty(&local)?)?;
    f.write(&auth, "{\"fixture\":true}\n")?;
    let web_search = f.home.join(".pi/agent/web-search.json");
    f.write(&web_search, "{\"exaApiKey\": \"neutral-key\"}\n")?;
    f.apply(&[], &[])?;
    assert!(!settings.is_symlink(), "Pi must own a writable local file");
    assert!(
        !web_search.is_symlink(),
        "pi-web-access must own a writable local file"
    );
    let expected_search: Value = serde_json::from_slice(&fs::read(
        f.source.join("home/pi/.pi/agent/web-search.json"),
    )?)?;
    let configured_search: Value = serde_json::from_slice(&fs::read(&web_search)?)?;
    assert_eq!(
        configured_search["searchRouting"],
        expected_search["searchRouting"]
    );
    assert_eq!(configured_search["exaApiKey"], "neutral-key");
    let configured: Value = serde_json::from_slice(&fs::read(&settings)?)?;
    for key in ["theme", "defaultTools", "enabledModels"] {
        assert_eq!(configured[key], expected[key], "deployed preference: {key}");
    }
    assert_eq!(
        configured["compaction"]["modelOverrides"],
        expected["compaction"]["modelOverrides"]
    );
    assert_eq!(configured["compaction"]["reserveTokens"], 16384);
    for key in [
        "deviceId",
        "packages",
        "defaultProvider",
        "defaultModel",
        "lastChangelogVersion",
        "unowned",
    ] {
        assert_eq!(configured[key], local[key], "machine-local setting: {key}");
    }
    let before = fs::read(&settings)?;
    let modified = fs::metadata(&settings)?.modified()?;
    f.apply(&[], &[])?;
    assert_eq!(fs::read(&settings)?, before, "repeat changed settings");
    assert_eq!(
        fs::metadata(&settings)?.modified()?,
        modified,
        "repeat rewrote satisfied settings"
    );

    let mut edited = configured;
    edited["theme"] = "local-edit".into();
    edited["deviceId"] = "new-neutral-device".into();
    f.write(&settings, &serde_json::to_string_pretty(&edited)?)?;
    f.apply(&[], &[])?;
    let reapplied: Value = serde_json::from_slice(&fs::read(&settings)?)?;
    assert_eq!(reapplied["theme"], expected["theme"]);
    assert_eq!(reapplied["deviceId"], edited["deviceId"]);

    f.write(&settings, "{invalid json\n")?;
    let refused = f.bootstrap(&[], &[])?;
    assert!(
        !refused.status.success(),
        "invalid settings accepted: {refused:?}"
    );
    assert_eq!(fs::read_to_string(&settings)?, "{invalid json\n");
    fs::remove_file(&settings)?;
    f.apply(&[], &[])?;
    let fresh: Value = serde_json::from_slice(&fs::read(&settings)?)?;
    assert_eq!(
        fresh, expected,
        "fresh settings differ from owned preferences"
    );
    assert_eq!(fs::metadata(&settings)?.permissions().mode() & 0o777, 0o600);
    assert_eq!(fs::read_to_string(&auth)?, "{\"fixture\":true}\n");
    assert_eq!(f.git(&f.checkout(), &["status", "--porcelain"])?, "");
    Ok(())
}

#[test]
fn acquired_sources_exclude_their_development_configuration() -> Result {
    let f = Fixture::new()?;
    f.apply(&[], &[])?;
    let env = f.ok(&f.checkout(), "mise", &["env", "--json"])?;
    assert!(
        !env.contains("PARENT_DEVELOPMENT_CONFIG"),
        "acquired source inherited its development configuration: {env}"
    );
    Ok(())
}

#[test]
fn private_module_selection_persists_without_cross_source_writes() -> Result {
    let f = Fixture::new()?;
    let private = f.private_source()?;
    f.stage_inputs(Some(&private))?;
    f.apply(&["-E", "private"], &[])?;
    f.apply(&[], &[])?;
    assert!(f.home.join(".config/mise/conf.d/dots-private").is_symlink());
    assert!(fs::read_to_string(f.home.join(".config/mise/miserc.local.toml"))?.contains("private"));
    assert!(
        f.home
            .join(".agents/skills/local-example/SKILL.md")
            .exists()
    );
    assert!(!f.checkout().join("skills/local-example").exists());
    // Public and private checkout rules share one directory.
    for rule in ["default.json", "example.json"] {
        assert!(f.home.join(".config/rex/checkouts").join(rule).is_symlink());
    }
    assert!(
        !f.checkout()
            .join("home/rex/.config/rex/checkouts/example.json")
            .exists()
    );
    assert_eq!(f.git(&f.checkout(), &["status", "--porcelain"])?, "");
    assert_eq!(
        f.git(&f.checkout().join("private"), &["status", "--porcelain"])?,
        ""
    );
    Ok(())
}

#[test]
fn first_use_preview_stops_at_uncloned_source_without_activation() -> Result {
    let f = Fixture::new()?;
    let output = f.bootstrap(&[], &["--dry-run"])?;
    assert!(
        !output.status.success(),
        "uncloned configuration was evaluated: {output:?}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("sources do not exist"));
    assert!(!f.checkout().exists());
    assert!(!f.home.join(".config/mise/conf.d/dots").exists());
    assert!(!f.home.join(".agents/skills").exists());
    Ok(())
}

#[test]
fn canonical_source_reuses_native_answers_without_transfer_staging() -> Result {
    let f = Fixture::new()?;
    let private = f.private_source()?;
    f.stage_inputs(Some(&private))?;
    f.apply(&["-E", "private"], &[])?;
    fs::remove_dir_all(&f.stage)?;
    f.ok(
        &f.checkout().join("bootstrap/source"),
        "mise",
        &["bootstrap", "--only", "files,repos,dotfiles"],
    )?;
    assert_eq!(f.git(&f.checkout(), &["status", "--porcelain"])?, "");
    assert_eq!(
        f.git(&f.checkout().join("private"), &["status", "--porcelain"])?,
        ""
    );
    Ok(())
}

#[test]
fn matching_directory_link_unfolds_without_writing_source() -> Result {
    let f = Fixture::new()?;
    fs::create_dir_all(f.checkout().parent().ok_or("checkout parent")?)?;
    f.git(
        &f.root,
        &[
            "clone",
            f.source.to_str().ok_or("source path")?,
            f.checkout().to_str().ok_or("checkout path")?,
        ],
    )?;
    fs::create_dir_all(f.home.join(".config"))?;
    unix_fs::symlink(
        f.checkout().join("home/rex/.config/rex"),
        f.home.join(".config/rex"),
    )?;
    f.apply(&[], &[])?;
    assert!(!f.home.join(".config/rex").is_symlink());
    assert!(f.home.join(".config/rex/init.lua").is_symlink());
    assert_eq!(f.git(&f.checkout(), &["status", "--porcelain"])?, "");
    Ok(())
}

#[test]
fn generated_unowned_skills_remain_outside_source() -> Result {
    let f = Fixture::new()?;
    let generated = f.home.join(".local/share/dots/skills/rex/SKILL.md");
    f.write(&generated, "generated by the host\n")?;
    f.apply(&[], &[])?;
    assert_eq!(fs::read_to_string(&generated)?, "generated by the host\n");
    assert!(f.home.join(".agents/skills/rex/SKILL.md").exists());
    assert!(!f.checkout().join("skills/rex").exists());
    Ok(())
}

#[test]
fn duplicate_public_private_skill_claims_fail_before_discovery_is_installed() -> Result {
    let f = Fixture::new()?;
    let private = f.private_source()?;
    f.write(
        &private.join("skills/public-example/SKILL.md"),
        "conflicting skill\n",
    )?;
    f.commit(&private, "Conflicting claim")?;
    f.stage_inputs(Some(&private))?;
    let output = f.bootstrap(&["-E", "private"], &[])?;
    assert!(
        !output.status.success(),
        "duplicate ownership accepted: {output:?}"
    );
    assert!(!f.home.join(".agents/skills").exists());
    assert_eq!(f.git(&f.checkout(), &["status", "--porcelain"])?, "");
    Ok(())
}

#[test]
fn apply_retains_source_and_update_advances_the_deployed_configuration() -> Result {
    let f = Fixture::new()?;
    f.apply(&[], &[])?;
    let first = f.git(&f.checkout(), &["rev-parse", "HEAD"])?;
    f.write(
        &f.source.join("skills/later/SKILL.md"),
        "---\nname: later\ndescription: Later skill\n---\nLater.\n",
    )?;
    f.commit(&f.source, "Add selected skill")?;
    f.apply(&[], &[])?;
    assert_eq!(f.git(&f.checkout(), &["rev-parse", "HEAD"])?, first);
    assert!(!f.home.join(".agents/skills/later").exists());
    f.apply(&[], &["--update"])?;
    assert_eq!(
        f.git(&f.checkout(), &["rev-parse", "HEAD"])?,
        f.git(&f.source, &["rev-parse", "HEAD"])?
    );
    assert!(f.home.join(".agents/skills/later/SKILL.md").exists());
    Ok(())
}

#[test]
fn dirty_sources_stop_the_operation_without_resetting_work() -> Result {
    let f = Fixture::new()?;
    f.apply(&[], &[])?;
    f.write(
        &f.checkout()
            .join("home/worktrunk/.config/worktrunk/config.toml"),
        "# uncommitted work\n",
    )?;
    let output = f.bootstrap(&[], &["--update"])?;
    assert!(
        !output.status.success(),
        "dirty source was accepted: {output:?}"
    );
    assert_eq!(
        fs::read_to_string(
            f.checkout()
                .join("home/worktrunk/.config/worktrunk/config.toml")
        )?,
        "# uncommitted work\n"
    );
    Ok(())
}

#[test]
fn conflicting_origins_stop_before_another_source_is_created() -> Result {
    let f = Fixture::new()?;
    f.apply(&[], &[])?;
    f.git(
        &f.checkout(),
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.net/other.git",
        ],
    )?;
    let private = f.private_source()?;
    f.stage_inputs(Some(&private))?;
    let output = f.bootstrap(&["-E", "private"], &[])?;
    assert!(
        !output.status.success(),
        "conflicting origin was accepted: {output:?}"
    );
    assert!(!f.checkout().join("private").exists());
    assert_eq!(
        f.git(&f.checkout(), &["remote", "get-url", "origin"])?,
        "https://example.net/other.git"
    );
    Ok(())
}

#[test]
fn missing_private_input_is_a_native_required_variable_error() -> Result {
    let f = Fixture::new()?;
    let output = f.bootstrap(&["-E", "private"], &[])?;
    assert!(
        !output.status.success(),
        "missing private input was accepted: {output:?}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("dots_private_url"));
    assert!(!f.checkout().exists());
    Ok(())
}

#[test]
fn private_clone_failure_reports_partial_progress_without_installing_discovery() -> Result {
    let f = Fixture::new()?;
    f.stage_inputs(Some(&f.root.join("unavailable-private")))?;
    let output = f.bootstrap(&["-E", "private"], &[])?;
    assert!(
        !output.status.success(),
        "unavailable private source was accepted: {output:?}"
    );
    assert!(!f.home.join(".agents/skills").exists());
    assert!(!f.checkout().join("private").exists());
    Ok(())
}

#[test]
fn unmanaged_discovery_directory_is_preserved_on_failure() -> Result {
    let f = Fixture::new()?;
    f.write(
        &f.home.join(".agents/skills/user/SKILL.md"),
        "user content\n",
    )?;
    let output = f.bootstrap(&[], &[])?;
    assert!(
        !output.status.success(),
        "unmanaged directory was replaced: {output:?}"
    );
    assert!(!f.home.join(".agents/skills").is_symlink());
    assert_eq!(
        fs::read_to_string(f.home.join(".agents/skills/user/SKILL.md"))?,
        "user content\n"
    );
    Ok(())
}

#[test]
fn committed_candidates_use_persistent_native_owned_bundle_origins() -> Result {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new()?;
    let private = f.private_source()?;
    for (origin, name) in [(&f.source, "public.bundle"), (&private, "private.bundle")] {
        f.git(
            origin,
            &[
                "bundle",
                "create",
                f.stage.join(name).to_str().ok_or("bundle path")?,
                "HEAD",
                "refs/heads/main",
            ],
        )?;
    }
    let candidates = fs::read_to_string(f.stage.join("candidates.toml"))?;
    let current = fs::read_to_string(f.stage.join("mise.local.toml"))?;
    let inputs = candidates
        + &current[current
            .find("\n[tasks.bootstrap]")
            .ok_or("fixture handoff")?..];
    f.write(&f.stage.join("mise.local.toml"), &inputs)?;
    f.apply(&["-E", "private"], &[])?;
    let bundles = f.home.join(".local/share/dots/source-bundles");
    assert_eq!(
        f.git(&f.checkout(), &["remote", "get-url", "origin"])?,
        bundles
            .join("public.bundle")
            .to_str()
            .ok_or("public bundle path")?
    );
    assert_eq!(
        f.git(
            &f.checkout().join("private"),
            &["remote", "get-url", "origin"]
        )?,
        bundles
            .join("private.bundle")
            .to_str()
            .ok_or("private bundle path")?
    );
    assert_eq!(
        fs::metadata(bundles.join("public.bundle"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(fs::metadata(&bundles)?.permissions().mode() & 0o777, 0o700);
    Ok(())
}

#[test]
fn public_candidates_do_not_require_or_transfer_a_private_bundle() -> Result {
    let f = Fixture::new()?;
    f.git(
        &f.source,
        &[
            "bundle",
            "create",
            f.stage
                .join("public.bundle")
                .to_str()
                .ok_or("bundle path")?,
            "HEAD",
            "refs/heads/main",
        ],
    )?;
    let candidates = fs::read_to_string(f.stage.join("candidates.toml"))?;
    let current = fs::read_to_string(f.stage.join("mise.local.toml"))?;
    f.write(
        &f.stage.join("mise.local.toml"),
        &(candidates
            + &current[current
                .find("\n[tasks.bootstrap]")
                .ok_or("fixture handoff")?..]),
    )?;
    f.apply(&[], &[])?;
    assert!(
        !f.home
            .join(".local/share/dots/source-bundles/private.bundle")
            .exists()
    );
    assert!(!f.checkout().join("private").exists());
    Ok(())
}
