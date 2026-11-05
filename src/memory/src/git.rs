//! Controlled local Git plumbing; subprocess lifetimes and I/O belong to dots-process.
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use crate::{
    Error, MAX_CORPUS_BYTES, MAX_DOCUMENTS, MAX_METADATA_BYTES, Result, Snapshot, invalid,
};

pub(crate) const HEAD: &str = "refs/heads/knowledge";
const INPUT_LIMIT: usize = 2 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(15);
const CONFIGURATION: &[(&str, &str)] = &[
    ("core.repositoryformatversion", "1"),
    ("extensions.objectformat", "sha256"),
    ("core.bare", "true"),
    ("core.fsync", "all"),
    ("core.fsyncmethod", "fsync"),
    ("core.hookspath", "/dev/null"),
    ("core.logallrefupdates", "always"),
    ("gc.auto", "0"),
    ("maintenance.auto", "false"),
    ("receive.autogc", "false"),
    ("receive.denynonfastforwards", "true"),
    ("receive.denydeletes", "true"),
    ("receive.hiderefs", HEAD),
    ("fetch.fsckobjects", "true"),
    ("transfer.fsckobjects", "true"),
];

#[derive(Clone, Debug)]
pub(crate) struct Git {
    root: PathBuf,
    actor: String,
}

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub oid: String,
    pub size: usize,
}

impl Git {
    pub(crate) fn new(root: PathBuf, actor: String) -> Self {
        Self { root, actor }
    }

    fn command(&self, args: &[&str]) -> Result<Command> {
        let mut command = Command::new("git");
        command
            .env_clear()
            .env(
                "PATH",
                env::var_os("PATH").ok_or_else(|| invalid("PATH is unavailable"))?,
            )
            .env("HOME", &self.root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_ALLOW_PROTOCOL", "file")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_NAME", &self.actor)
            .env("GIT_AUTHOR_EMAIL", "memory@localhost")
            .env("GIT_COMMITTER_NAME", "dots memory")
            .env("GIT_COMMITTER_EMAIL", "memory@localhost")
            .env("TZ", "UTC")
            .env("LC_ALL", "C")
            .current_dir(&self.root)
            .args([
                "--no-pager",
                "--no-replace-objects",
                "--literal-pathspecs",
                "--git-dir",
            ])
            .arg(self.root.join("repo.git"))
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsync=all",
                "-c",
                "core.fsyncMethod=fsync",
                "-c",
                "gc.auto=0",
                "-c",
                "maintenance.auto=false",
                "-c",
                "commit.gpgSign=false",
            ])
            .args(args);
        Ok(command)
    }

    pub(crate) fn run(
        &self,
        args: &[&str],
        input: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<Output> {
        let operation = operation(args);
        let mut runner =
            dots_process::Runner::new(DEADLINE, MAX_METADATA_BYTES).stdout_limit(limit);
        if let Some(input) = input {
            runner = runner.stdin(input, INPUT_LIMIT);
        }
        runner
            .output(self.command(args)?)
            .map_err(|source| Error::Process { operation, source })
    }

    pub(crate) fn bytes(
        &self,
        args: &[&str],
        input: Option<Vec<u8>>,
        limit: usize,
    ) -> Result<Vec<u8>> {
        let output = self.run(args, input, limit)?;
        if !output.status.success() {
            return Err(failure(args, &output));
        }
        Ok(output.stdout)
    }

    pub(crate) fn text(&self, args: &[&str], input: Option<Vec<u8>>) -> Result<String> {
        String::from_utf8(self.bytes(args, input, MAX_METADATA_BYTES)?)
            .map_err(|_| invalid("non-UTF-8 Git metadata"))
    }

    pub(crate) fn initialize(&self) -> Result<()> {
        self.text(
            &[
                "init",
                "--bare",
                "--quiet",
                "--object-format=sha256",
                "--initial-branch=knowledge",
                "--template=",
            ],
            None,
        )?;
        for &(key, value) in CONFIGURATION {
            self.text(&["config", key, value], None)?;
        }
        self.validate_configuration()
    }

    pub(crate) fn validate_configuration(&self) -> Result<()> {
        for relative in [
            "repo.git/config",
            "repo.git/HEAD",
            "repo.git/objects",
            "repo.git/objects/pack",
            "repo.git/refs",
            "repo.git/refs/heads",
        ] {
            dots_documents::paths::resolve(&self.root, Path::new(relative))?;
        }
        let listing = self.text(
            &["config", "--local", "--no-includes", "--null", "--list"],
            None,
        )?;
        let mut required: BTreeMap<_, _> = CONFIGURATION.iter().copied().collect();
        let mut seen = BTreeSet::new();
        for record in listing.split('\0').filter(|record| !record.is_empty()) {
            let (key, value) = record
                .split_once('\n')
                .ok_or_else(|| invalid("Git configuration needs explicit values"))?;
            if !seen.insert(key) {
                return Err(invalid(format!(
                    "repeated knowledge Git configuration: {key}"
                )));
            }
            if let Some(expected) = required.remove(key) {
                if value != expected {
                    return Err(invalid(format!(
                        "knowledge Git configuration {key} must be {expected}"
                    )));
                }
            } else if !matches!(
                key,
                "core.filemode" | "core.ignorecase" | "core.precomposeunicode"
            ) || !matches!(value, "true" | "false")
            {
                return Err(invalid(format!(
                    "unexpected configuration in owned knowledge repository: {key}"
                )));
            }
        }
        if let Some((key, _)) = required.first_key_value() {
            return Err(invalid(format!(
                "missing knowledge Git configuration: {key}"
            )));
        }
        for relative in [
            "repo.git/shallow",
            "repo.git/commondir",
            "repo.git/objects/info/alternates",
            "repo.git/refs/replace",
        ] {
            if self
                .root
                .join(relative)
                .symlink_metadata()
                .map(|_| true)
                .or_else(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound {
                        Ok(false)
                    } else {
                        Err(error)
                    }
                })
                .map_err(|e| crate::io("inspect Git storage", &self.root.join(relative), e))?
            {
                return Err(invalid(format!(
                    "unsupported indirection in owned knowledge repository: {relative}"
                )));
            }
        }
        if !self
            .text(
                &["for-each-ref", "--format=%(refname)", "refs/replace"],
                None,
            )?
            .is_empty()
        {
            return Err(invalid("replacement objects are not allowed"));
        }
        Ok(())
    }

    pub(crate) fn head(&self) -> Result<Snapshot> {
        dots_documents::paths::resolve(&self.root, Path::new("repo.git/refs/heads/knowledge"))?;
        let output = self.text(
            &[
                "for-each-ref",
                "--format=%(refname)%00%(objecttype)%00%(objectname)%00%(symref)",
                HEAD,
            ],
            None,
        )?;
        let fields: Vec<_> = output
            .strip_suffix('\n')
            .ok_or_else(|| invalid("published knowledge reference is absent"))?
            .split('\0')
            .collect();
        if fields.len() != 4 || fields[0] != HEAD || fields[1] != "commit" || !fields[3].is_empty()
        {
            return Err(invalid(
                "published knowledge reference must directly name one commit",
            ));
        }
        fields[2].parse()
    }

    pub(crate) fn is_ancestor(&self, ancestor: &Snapshot, descendant: &Snapshot) -> Result<bool> {
        let args = [
            "merge-base",
            "--is-ancestor",
            ancestor.as_str(),
            descendant.as_str(),
        ];
        let output = self.run(&args, None, MAX_METADATA_BYTES)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(failure(&args, &output)),
        }
    }

    pub(crate) fn publish(&self, expected: Option<&Snapshot>, proposed: &Snapshot) -> Result<()> {
        let args = [
            "update-ref",
            "--no-deref",
            HEAD,
            proposed.as_str(),
            expected.map_or("", Snapshot::as_str),
        ];
        let output = self.run(&args, None, MAX_METADATA_BYTES)?;
        if output.status.success() {
            return Ok(());
        }
        if let Some(expected) = expected {
            let actual = self.head()?;
            if actual != *expected {
                return Err(Error::Conflict {
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        Err(failure(&args, &output))
    }

    pub(crate) fn entries(&self, snapshot: &Snapshot) -> Result<BTreeMap<String, Entry>> {
        let listing = self.text(&["ls-tree", "-r", "-l", "-z", snapshot.as_str()], None)?;
        let mut entries = BTreeMap::new();
        let mut total = 0_usize;
        for record in listing.split('\0').filter(|record| !record.is_empty()) {
            let (metadata, path) = record
                .split_once('\t')
                .ok_or_else(|| invalid("invalid Git tree metadata"))?;
            let parts: Vec<_> = metadata.split_whitespace().collect();
            if parts.len() != 4 || parts[0] != "100644" || parts[1] != "blob" {
                return Err(invalid(format!(
                    "knowledge entry is not a regular document: {path}"
                )));
            }
            let oid: Snapshot = parts[2].parse()?;
            let size: usize = parts[3]
                .parse()
                .map_err(|_| invalid("invalid Git object size"))?;
            total = total
                .checked_add(size)
                .ok_or_else(|| invalid("knowledge corpus size overflow"))?;
            if size > crate::MAX_FILE_BYTES
                || total > MAX_CORPUS_BYTES
                || entries.len() >= MAX_DOCUMENTS
            {
                return Err(invalid("knowledge corpus exceeds storage bounds"));
            }
            if entries
                .insert(
                    path.into(),
                    Entry {
                        oid: oid.to_string(),
                        size,
                    },
                )
                .is_some()
            {
                return Err(invalid("duplicate Git tree path"));
            }
        }
        Ok(entries)
    }

    pub(crate) fn blobs(&self, ids: &[&str], maximum: usize) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut input = Vec::new();
        for id in ids {
            input.extend_from_slice(id.as_bytes());
            input.push(b'\n');
        }
        let bytes = self.bytes(&["cat-file", "--batch"], Some(input), maximum)?;
        let mut offset = 0_usize;
        let mut result = BTreeMap::new();
        for id in ids {
            let line_end = bytes
                .get(offset..)
                .and_then(|rest| rest.iter().position(|&byte| byte == b'\n'))
                .map(|length| offset + length)
                .ok_or_else(|| invalid("incomplete Git batch header"))?;
            let header = std::str::from_utf8(&bytes[offset..line_end])
                .map_err(|_| invalid("invalid Git batch header"))?;
            let fields: Vec<_> = header.split(' ').collect();
            if fields.len() != 3 || fields[0] != *id || fields[1] != "blob" {
                return Err(invalid("Git batch returned a missing or unexpected object"));
            }
            let size: usize = fields[2]
                .parse()
                .map_err(|_| invalid("invalid Git batch size"))?;
            let start = line_end + 1;
            let end = start
                .checked_add(size)
                .ok_or_else(|| invalid("Git batch length overflow"))?;
            let content = bytes
                .get(start..end)
                .ok_or_else(|| invalid("incomplete Git blob"))?;
            if bytes.get(end) != Some(&b'\n') {
                return Err(invalid("incomplete Git batch terminator"));
            }
            result.insert((*id).into(), content.to_vec());
            offset = end + 1;
        }
        if offset != bytes.len() {
            return Err(invalid("unexpected trailing Git batch output"));
        }
        Ok(result)
    }

    pub(crate) fn blob(&self, oid: &str, size: usize) -> Result<Vec<u8>> {
        self.blobs(&[oid], size + 128)?
            .remove(oid)
            .ok_or_else(|| invalid("missing requested Git blob"))
    }

    pub(crate) fn write_blob(&self, content: &[u8]) -> Result<String> {
        let value = self.text(
            &["hash-object", "-w", "--stdin", "--no-filters"],
            Some(content.into()),
        )?;
        Ok(value.trim().parse::<Snapshot>()?.to_string())
    }

    pub(crate) fn tree(&self, entries: &BTreeMap<String, Entry>) -> Result<String> {
        let mut directories: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        directories.entry(String::new()).or_default();
        for (path, entry) in entries {
            let (directory, name) = path.rsplit_once('/').unwrap_or(("", path));
            directories
                .entry(directory.into())
                .or_default()
                .extend_from_slice(format!("100644 blob {}\t{name}\0", entry.oid).as_bytes());
        }
        let depth = |path: &str| {
            if path.is_empty() {
                0
            } else {
                path.split('/').count()
            }
        };
        loop {
            let deepest = directories
                .keys()
                .map(|path| depth(path))
                .max()
                .expect("root remains until it is written");
            let names: Vec<_> = directories
                .keys()
                .filter(|path| depth(path) == deepest)
                .cloned()
                .collect();
            let mut input = Vec::new();
            for name in &names {
                input.extend(directories.remove(name).expect("selected directory exists"));
                input.push(0);
            }
            let output = self.text(&["mktree", "--batch", "-z"], Some(input))?;
            let objects: Vec<_> = output
                .lines()
                .map(str::parse::<Snapshot>)
                .collect::<Result<_>>()?;
            if objects.len() != names.len() {
                return Err(invalid(
                    "Git tree batch returned an unexpected object count",
                ));
            }
            for (directory, oid) in names.into_iter().zip(objects) {
                if directory.is_empty() {
                    return Ok(oid.to_string());
                }
                let (parent, name) = directory.rsplit_once('/').unwrap_or(("", &directory));
                directories
                    .entry(parent.into())
                    .or_default()
                    .extend_from_slice(format!("040000 tree {oid}\t{name}\0").as_bytes());
            }
        }
    }

    pub(crate) fn commit(
        &self,
        tree: &str,
        parents: &[&Snapshot],
        message: &str,
    ) -> Result<Snapshot> {
        let mut args = vec!["commit-tree", tree];
        for parent in parents {
            args.extend(["-p", parent.as_str()]);
        }
        args.extend(["-m", message]);
        self.text(&args, None)?.trim().parse()
    }
}

fn operation(args: &[&str]) -> &'static str {
    match args.first().copied() {
        Some("update-ref") => "publish knowledge reference",
        Some("cat-file") => "read knowledge objects",
        Some("mktree" | "hash-object" | "commit-tree") => "stage knowledge objects",
        _ => "inspect knowledge repository",
    }
}

fn failure(args: &[&str], output: &Output) -> Error {
    Error::Git {
        operation: operation(args),
        status: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
    }
}
