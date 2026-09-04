use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::SystemTime;

use anyhow::{Context as _, Result, bail};
use herdrkit::Theme;
use herdrkit::picker::{Cell, Column, Entry, Group};
use serde::Deserialize;

/// Recently modified changes are worth highlighting, but "changed" includes
/// every untracked file, and in a checkout with uncommitted directories that
/// can be most of the repo. Past this many the overflow remains reachable in
/// the other-files group.
const MARKED: usize = 20;

const MARK: usize = 2;
const DIFFSTAT: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Changed or untracked files with the newest modification times.
    RecentChanges,
    /// Every remaining tracked, changed, or untracked file.
    OtherFiles,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: PathBuf,
    tier: Tier,
    /// Repo-relative, which is both what is drawn and what is searched: an
    /// absolute path would put the same leading directories in every row.
    relative: PathBuf,
    diffstat: Option<String>,
}

impl Entry for File {
    type Group = Tier;

    fn group(&self) -> Tier {
        self.tier
    }

    fn cells(&self, theme: &Theme) -> Vec<Cell> {
        let relative = self.relative.to_string_lossy().into_owned();
        match self.tier {
            Tier::RecentChanges => vec![
                Cell::tag("●", theme.green),
                Cell::new(relative, theme.strong),
                Cell::tag(self.diffstat.clone().unwrap_or_default(), theme.muted),
            ],
            Tier::OtherFiles => vec![
                Cell::tag("", theme.muted),
                Cell::new(relative, theme.strong),
            ],
        }
    }
}

pub fn groups() -> Vec<Group<Tier>> {
    vec![
        Group::new(
            Tier::RecentChanges,
            "recent changes",
            vec![Column::new(MARK), Column::fill(), Column::new(DIFFSTAT)],
        ),
        Group::new(
            Tier::OtherFiles,
            "other files",
            vec![Column::new(MARK), Column::fill()],
        ),
    ]
}

/// Everything the index is built from, in one value, so the transform below is
/// a pure function over a fixture.
#[derive(Debug, Default, Deserialize)]
pub struct Listings {
    /// Tracked files with uncommitted changes.
    pub changed: Vec<PathBuf>,
    /// Untracked files git is not ignoring.
    pub untracked: Vec<PathBuf>,
    /// Everything git tracks, changed or not.
    pub tracked: Vec<PathBuf>,
    /// `added`, `removed`, `path` per changed tracked file.
    pub numstat: Vec<Numstat>,
    /// Modification times, newest first — the caller's, because reading them
    /// is I/O. A path absent here has been deleted since git listed it.
    pub modification_times: Vec<ModificationTime>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Numstat {
    pub added: String,
    pub removed: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModificationTime {
    pub path: PathBuf,
    /// Seconds since the epoch. Only the ordering is used.
    pub at: u64,
}

impl Listings {
    pub fn collect(root: &Path) -> Result<Self> {
        let git = Git { root };
        git.ensure_worktree()?;
        let has_head = git.has_head()?;
        let changed = git.changed(has_head)?;
        let untracked = git.nul_paths(&["ls-files", "-z", "--others", "--exclude-standard"])?;

        let mut candidates: Vec<&PathBuf> = changed.iter().chain(&untracked).collect();
        candidates.sort_unstable();
        candidates.dedup();
        // One stat per candidate rather than a process per file: spawning
        // `stat` a hundred times cost most of the time to first paint. A path
        // that no longer exists fails here and drops out, which is the
        // existence check too.
        let mut modification_times: Vec<ModificationTime> = candidates
            .into_iter()
            .filter_map(|relative| {
                let at = std::fs::symlink_metadata(root.join(relative))
                    .and_then(|meta| meta.modified())
                    .ok()?
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .ok()?
                    .as_secs();
                Some(ModificationTime {
                    path: relative.clone(),
                    at,
                })
            })
            .collect();
        modification_times.sort_by(|a, b| b.at.cmp(&a.at).then(a.path.cmp(&b.path)));

        Ok(Self {
            changed,
            untracked,
            tracked: git.nul_paths(&["ls-files", "-z"])?,
            numstat: git.numstat(has_head)?,
            modification_times,
        })
    }

    /// Two tiers over one checkout: the newest changes first, then every
    /// remaining tracked, changed, or untracked file.
    pub fn index(&self, root: &Path) -> Vec<File> {
        let diffstat = |relative: &Path| {
            self.numstat
                .iter()
                .find(|stat| stat.path == relative)
                .map(|stat| format!("+{}/-{}", stat.added, stat.removed))
        };

        let mut out = Vec::new();
        let mut marked: HashSet<&Path> = HashSet::new();
        for entry in self.modification_times.iter().take(MARKED) {
            marked.insert(&entry.path);
            out.push(File {
                path: root.join(&entry.path),
                tier: Tier::RecentChanges,
                relative: entry.path.clone(),
                diffstat: diffstat(&entry.path),
            });
        }

        // The overflow is appended because `ls-files` omits untracked files
        // entirely, and dropping them here would make a file just created
        // unreachable once it fell past the cap.
        let overflow = self
            .modification_times
            .iter()
            .skip(MARKED)
            .map(|entry| &entry.path);
        let mut seen: HashSet<&Path> = HashSet::new();
        for relative in self.tracked.iter().chain(overflow) {
            if marked.contains(relative.as_path()) || !seen.insert(relative) {
                continue;
            }
            out.push(File {
                path: root.join(relative),
                tier: Tier::OtherFiles,
                relative: relative.clone(),
                diffstat: None,
            });
        }
        out
    }
}

struct Git<'a> {
    root: &'a Path,
}

impl Git<'_> {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(self.root)
            .args(args)
            // A listing must not rewrite .git/index: everything watching the
            // repo treats that write as a change.
            .env("GIT_OPTIONAL_LOCKS", "0");
        command
    }

    fn output(&self, args: &[&str]) -> Result<Output> {
        let output = self
            .command(args)
            .output()
            .with_context(|| format!("running git {}", args.join(" ")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "git {} in {}: {}",
                args.join(" "),
                self.root.display(),
                stderr.trim()
            );
        }
        Ok(output)
    }

    fn ensure_worktree(&self) -> Result<()> {
        let output = self.output(&["rev-parse", "--is-inside-work-tree"])?;
        if output.stdout.strip_suffix(b"\n") != Some(b"true") {
            bail!("{} is not inside a git worktree", self.root.display());
        }
        Ok(())
    }

    fn has_head(&self) -> Result<bool> {
        let output = self
            .command(&["rev-parse", "--verify", "--quiet", "HEAD"])
            .output()
            .context("checking whether the repository has a commit")?;
        if output.status.success() {
            return Ok(true);
        }
        if output.status.code() == Some(1) {
            return Ok(false);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "checking HEAD in {}: {}",
            self.root.display(),
            stderr.trim()
        )
    }

    fn changed(&self, has_head: bool) -> Result<Vec<PathBuf>> {
        if has_head {
            self.nul_paths(&["diff", "--name-only", "-z", "HEAD", "--"])
        } else {
            self.nul_paths(&["diff", "--cached", "--name-only", "-z", "--"])
        }
    }

    /// NUL-delimited path output is the only Git format in which newlines and
    /// non-UTF-8 bytes are unambiguous and preserved.
    fn nul_paths(&self, args: &[&str]) -> Result<Vec<PathBuf>> {
        let output = self.output(args)?;
        parse_nul_paths(&output.stdout).with_context(|| format!("parsing git {}", args.join(" ")))
    }

    /// One numstat call for every stat, rather than one per row.
    fn numstat(&self, has_head: bool) -> Result<Vec<Numstat>> {
        let output = if has_head {
            self.output(&["diff", "--numstat", "-z", "HEAD", "--"])?
        } else {
            self.output(&["diff", "--cached", "--numstat", "-z", "--"])?
        };
        parse_numstat(&output.stdout).context("parsing git diff --numstat -z")
    }
}

fn parse_nul_paths(bytes: &[u8]) -> Result<Vec<PathBuf>> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        bail!("NUL-delimited path output had an unterminated final path");
    }
    Ok(bytes
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(OsString::from_vec(path.to_vec())))
        .collect())
}

fn parse_numstat(bytes: &[u8]) -> Result<Vec<Numstat>> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        bail!("NUL-delimited numstat output had an unterminated final record");
    }

    let mut records = bytes.split(|byte| *byte == 0);
    let mut out = Vec::new();
    while let Some(record) = records.next() {
        if record.is_empty() {
            break;
        }
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let added = fields.next().context("numstat record had no added count")?;
        let removed = fields
            .next()
            .context("numstat record had no removed count")?;
        let path = fields.next().context("numstat record had no path")?;

        // For a rename/copy, Git emits an empty path followed by old and new
        // path records. The new path is the file the picker can open.
        let path = if path.is_empty() {
            let _old = records
                .next()
                .context("renamed numstat record had no old path")?;
            records
                .next()
                .context("renamed numstat record had no new path")?
        } else {
            path
        };
        let added = std::str::from_utf8(added).context("numstat added count was not UTF-8")?;
        let removed =
            std::str::from_utf8(removed).context("numstat removed count was not UTF-8")?;
        out.push(Numstat {
            added: added.to_owned(),
            removed: removed.to_owned(),
            path: PathBuf::from(OsString::from_vec(path.to_vec())),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::ffi::OsStrExt as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

    struct TempRepo(PathBuf);

    impl TempRepo {
        fn unborn() -> Self {
            let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "herdr-nvim-files-test-{}-{sequence}",
                std::process::id()
            ));
            let status = Command::new("git")
                .args(["init", "--quiet"])
                .arg(&root)
                .status()
                .unwrap();
            assert!(status.success());
            Self(root)
        }

        fn add(&self, relative: &Path, contents: &[u8]) {
            fs::write(self.0.join(relative), contents).unwrap();
            let status = Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["add", "--"])
                .arg(relative)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    impl Drop for TempRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn listings() -> Listings {
        serde_json::from_str(include_str!("../tests/fixtures/listings.json")).unwrap()
    }

    fn index() -> Vec<File> {
        listings().index(Path::new("/repo"))
    }

    fn relatives(tier: Tier) -> Vec<PathBuf> {
        index()
            .into_iter()
            .filter(|file| file.tier == tier)
            .map(|file| file.relative)
            .collect()
    }

    #[test]
    fn recent_changes_come_first_and_newest_first() {
        assert_eq!(
            relatives(Tier::RecentChanges),
            [
                PathBuf::from("src/main.rs"),
                PathBuf::from("src/lib.rs"),
                PathBuf::from("notes.md")
            ]
        );
    }

    #[test]
    fn an_untracked_file_is_recent_even_though_ls_files_omits_it() {
        assert!(relatives(Tier::RecentChanges).contains(&PathBuf::from("notes.md")));
        assert!(!relatives(Tier::OtherFiles).contains(&PathBuf::from("notes.md")));
    }

    /// `deleted.rs` is in `changed` but has no mtime, so nothing stat'd it.
    /// It stays reachable because git still tracks it, but it cannot claim to
    /// be one of the recently modified files.
    #[test]
    fn a_changed_file_that_no_longer_exists_is_not_marked_as_recent() {
        assert!(!relatives(Tier::RecentChanges).contains(&PathBuf::from("deleted.rs")));
        assert!(relatives(Tier::OtherFiles).contains(&PathBuf::from("deleted.rs")));
    }

    #[test]
    fn a_recent_change_is_not_repeated_in_the_other_files_tier() {
        let all: Vec<PathBuf> = index().into_iter().map(|file| file.relative).collect();
        let mut unique = all.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(all.len(), unique.len(), "{all:?}");
    }

    #[test]
    fn only_a_tracked_change_carries_a_diffstat() {
        let stats: Vec<Option<String>> = index()
            .into_iter()
            .filter(|file| file.tier == Tier::RecentChanges)
            .map(|file| file.diffstat)
            .collect();
        assert_eq!(
            stats,
            [
                Some("+13/-1".to_owned()),
                Some("+50/-3".to_owned()),
                None // untracked, so git reports no numstat for it
            ]
        );
    }

    #[test]
    fn paths_are_resolved_against_the_repo_root() {
        assert_eq!(
            index().first().map(|file| file.path.clone()),
            Some(PathBuf::from("/repo/src/main.rs"))
        );
    }

    /// Past the cap a file stays reachable, just unmarked and further down.
    #[test]
    fn the_overflow_past_the_cap_lands_in_the_other_files_tier() {
        let many: Vec<ModificationTime> = (0..MARKED + 3)
            .map(|n| ModificationTime {
                path: PathBuf::from(format!("f{n}.rs")),
                at: u64::try_from(MARKED + 3 - n).unwrap_or(0),
            })
            .collect();
        let listings = Listings {
            untracked: many.iter().map(|m| m.path.clone()).collect(),
            modification_times: many,
            ..Listings::default()
        };
        let index = listings.index(Path::new("/repo"));
        assert_eq!(
            index
                .iter()
                .filter(|f| f.tier == Tier::RecentChanges)
                .count(),
            MARKED
        );
        assert_eq!(
            index.iter().filter(|f| f.tier == Tier::OtherFiles).count(),
            3
        );
        assert_eq!(index.len(), MARKED + 3);
    }

    #[test]
    fn an_empty_checkout_yields_an_empty_index() {
        assert!(Listings::default().index(Path::new("/repo")).is_empty());
    }

    #[test]
    fn nul_path_parser_preserves_newlines_and_non_utf8_bytes() {
        let paths = parse_nul_paths(b"normal.rs\0line\nbreak.rs\0bad-\xff.rs\0").unwrap();
        assert_eq!(
            paths,
            [
                PathBuf::from("normal.rs"),
                PathBuf::from("line\nbreak.rs"),
                PathBuf::from(OsString::from_vec(b"bad-\xff.rs".to_vec()))
            ]
        );
    }

    #[test]
    fn numstat_parser_uses_the_new_name_of_a_rename() {
        let stats = parse_numstat(b"2\t1\t\0old\nname.rs\0new-\xff.rs\0").unwrap();
        let stat = stats.first().unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stat.added, "2");
        assert_eq!(stat.removed, "1");
        assert_eq!(stat.path.as_os_str().as_bytes(), b"new-\xff.rs");
    }

    #[test]
    fn collection_handles_an_unborn_repo_and_a_newline_in_a_filename() {
        let repo = TempRepo::unborn();
        let relative = PathBuf::from("line\nbreak.rs");
        repo.add(&relative, b"first line\nsecond line\n");

        let listings = Listings::collect(&repo.0).unwrap();
        assert_eq!(listings.changed.as_slice(), std::slice::from_ref(&relative));
        assert_eq!(listings.tracked.as_slice(), std::slice::from_ref(&relative));
        assert_eq!(
            listings.numstat.first().map(|stat| &stat.path),
            Some(&relative)
        );
        let absolute = repo.0.join(&relative);
        assert_eq!(
            listings.index(&repo.0).first().map(|file| &file.path),
            Some(&absolute)
        );
    }

    #[test]
    fn collection_keeps_an_untracked_dangling_symlink_reachable() {
        use std::os::unix::fs::symlink;

        let repo = TempRepo::unborn();
        let relative = PathBuf::from("dangling-link");
        symlink("missing-target", repo.0.join(&relative)).unwrap();

        let listings = Listings::collect(&repo.0).unwrap();
        assert_eq!(
            listings.untracked.as_slice(),
            std::slice::from_ref(&relative)
        );
        assert_eq!(
            listings.modification_times.first().map(|entry| &entry.path),
            Some(&relative)
        );
        let absolute = repo.0.join(relative);
        assert_eq!(
            listings.index(&repo.0).first().map(|file| &file.path),
            Some(&absolute)
        );
    }

    #[test]
    fn the_declared_groups_accept_every_row() {
        herdrkit::picker::Picker::new(index())
            .groups(groups())
            .build()
            .expect("groups and cells agree");
    }
}
