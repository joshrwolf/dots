//! A worktree's review: draft and sent comments, files marked reviewed, and
//! the contents last sent to the agent. It lives in one small versioned
//! file in the worktree's own Git directory, written atomically; the texts
//! of what was sent sit beside it so "since last send" can diff them.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::repo::{FileChange, Reader, Status};

const VERSION: u32 = 1;
/// How far from its old place an anchor's text is looked for.
const REANCHOR_WINDOW: u32 = 400;
/// Lines of anchored code quoted in a message.
const EXCERPT_LINES: usize = 8;
const EXCERPT_WIDTH: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Side {
    Old,
    New,
}

/// What a comment is about, cycled in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    #[default]
    Comment,
    Issue,
    Question,
    Nit,
}

impl Kind {
    pub(crate) fn next(self) -> Self {
        match self {
            Kind::Comment => Kind::Issue,
            Kind::Issue => Kind::Question,
            Kind::Question => Kind::Nit,
            Kind::Nit => Kind::Comment,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Kind::Comment => "comment",
            Kind::Issue => "issue",
            Kind::Question => "question",
            Kind::Nit => "nit",
        }
    }
}

/// A range of lines on one side of a file, 0-based and inclusive, with
/// their text when anchored, to find them again after the file changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Lines {
    pub(crate) side: Side,
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) text: Vec<String>,
}

/// Where a comment or note sits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "at", rename_all = "lowercase")]
pub(crate) enum Place {
    /// The review as a whole.
    Review,
    File {
        path: String,
    },
    Lines {
        path: String,
        lines: Lines,
    },
}

impl Place {
    pub(crate) fn path(&self) -> Option<&str> {
        match self {
            Place::Review => None,
            Place::File { path } | Place::Lines { path, .. } => Some(path),
        }
    }

    pub(crate) fn lines(&self) -> Option<&Lines> {
        match self {
            Place::Lines { lines, .. } => Some(lines),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Comment {
    pub(crate) id: u64,
    pub(crate) place: Place,
    #[serde(default)]
    pub(crate) kind: Kind,
    pub(crate) text: String,
    /// Replacement code for the anchored lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) suggestion: Option<String>,
    /// Sent in the last round; shown dimmed until the next send.
    #[serde(default)]
    pub(crate) sent: bool,
    /// Its lines changed and their text was not found again.
    #[serde(default)]
    pub(crate) outdated: bool,
    /// The content of the anchored side when it was placed or last
    /// re-anchored: `None` until known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<String>,
}

/// A file marked reviewed, at the content it had.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Mark {
    pub(crate) path: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct State {
    pub(crate) version: u32,
    #[serde(default)]
    pub(crate) next_id: u64,
    #[serde(default)]
    pub(crate) comments: Vec<Comment>,
    #[serde(default)]
    pub(crate) marks: Vec<Mark>,
    /// Per file, the content of its new side when the last review was sent.
    #[serde(default)]
    pub(crate) seen: BTreeMap<String, String>,
    /// The Pi session the user picked for this worktree, by session id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: VERSION,
            next_id: 1,
            comments: Vec::new(),
            marks: Vec::new(),
            seen: BTreeMap::new(),
            agent: None,
        }
    }
}

/// A content id as stored: the blob id, or `missing` for no file.
pub(crate) fn content_key(id: Option<gix::ObjectId>) -> String {
    id.map_or_else(|| "missing".to_owned(), |id| id.to_string())
}

/// The directory holding a worktree's review state.
pub(crate) fn dir(git_dir: &Path) -> PathBuf {
    git_dir.join("dots-diff")
}

/// Where texts of sent content are kept, by blob id.
pub(crate) fn seen_dir(git_dir: &Path) -> PathBuf {
    dir(git_dir).join("seen")
}

impl State {
    /// The state in `git_dir`, or a fresh one when there is none. A file of
    /// another version or shape is an error rather than silently replaced.
    pub(crate) fn load(git_dir: &Path) -> Result<Self> {
        let path = dir(git_dir).join("state.json");
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let state: State = serde_json::from_slice(&bytes)
            .with_context(|| format!("reading {}", path.display()))?;
        anyhow::ensure!(
            state.version == VERSION,
            "{} has version {}, expected {VERSION}",
            path.display(),
            state.version
        );
        Ok(state)
    }

    pub(crate) fn save(&self, git_dir: &Path) -> Result<()> {
        let dir = dir(git_dir);
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let path = dir.join("state.json");
        let tmp = dir.join("state.json.tmp");
        let bytes = serde_json::to_vec_pretty(self)?;
        std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub(crate) fn add(
        &mut self,
        place: Place,
        kind: Kind,
        text: String,
        suggestion: Option<String>,
    ) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.comments.push(Comment {
            id,
            place,
            kind,
            text,
            suggestion,
            sent: false,
            outdated: false,
            content: None,
        });
        id
    }

    pub(crate) fn drafts(&self) -> impl Iterator<Item = &Comment> {
        self.comments.iter().filter(|c| !c.sent)
    }

    pub(crate) fn reviewed(&self, path: &str, content: &str) -> bool {
        self.marks
            .iter()
            .any(|m| m.path == path && m.content == content)
    }

    /// Marks or unmarks a file reviewed at its current content. Marks of
    /// the same file at other contents are dropped either way.
    pub(crate) fn toggle_reviewed(&mut self, path: &str, content: &str) -> bool {
        let was = self.reviewed(path, content);
        self.marks.retain(|m| m.path != path);
        if !was {
            self.marks.push(Mark {
                path: path.to_owned(),
                content: content.to_owned(),
            });
        }
        !was
    }

    /// After a send: drafts become sent, comments sent before are dropped,
    /// and `seen` records what the user was shown.
    pub(crate) fn sent(&mut self, ids: &[u64], seen: BTreeMap<String, String>) {
        self.comments.retain(|c| !c.sent);
        for c in &mut self.comments {
            if ids.contains(&c.id) {
                c.sent = true;
            }
        }
        self.seen = seen;
    }

    /// A file's side changed to `content`, whose lines are `lines`:
    /// comments anchored there move with their text, or become outdated;
    /// sent ones are dropped.
    pub(crate) fn file_changed(&mut self, path: &str, side: Side, content: &str, lines: &[&str]) {
        self.comments.retain_mut(|c| {
            let Place::Lines {
                path: p,
                lines: anchored,
            } = &mut c.place
            else {
                return true;
            };
            if p != path || anchored.side != side || c.content.as_deref() == Some(content) {
                return true;
            }
            if c.content.is_none() {
                // Placed before the content was known.
                c.content = Some(content.to_owned());
                return true;
            }
            if c.sent {
                return false;
            }
            match reanchor(anchored, lines) {
                Some(start) => {
                    let len = anchored.end - anchored.start;
                    anchored.start = start;
                    anchored.end = start + len;
                    c.outdated = false;
                }
                None => c.outdated = true,
            }
            c.content = Some(content.to_owned());
            true
        });
    }
}

/// Where `anchored`'s text starts in `lines`, searching outward from where
/// it was, or `None` when it is gone.
pub(crate) fn reanchor(anchored: &Lines, lines: &[&str]) -> Option<u32> {
    let want = &anchored.text;
    let n = u32::try_from(lines.len()).ok()?;
    let len = u32::try_from(want.len()).ok()?;
    if len == 0 || len > n {
        return None;
    }
    let matches = |at: u32| {
        want.iter()
            .zip(&lines[at as usize..(at + len) as usize])
            .all(|(a, b)| a == b)
    };
    let last = n - len;
    let from = anchored.start.min(last);
    for d in 0..=REANCHOR_WINDOW {
        if from + d <= last && matches(from + d) {
            return Some(from + d);
        }
        if d > 0 && d <= from && matches(from - d) {
            return Some(from - d);
        }
    }
    None
}

/// The changes since the last send: files sent before compare with what
/// was sent, and those unchanged since are left out. Files never sent keep
/// their base. A file whose content cannot be read keeps its change.
pub(crate) fn since_sent(
    files: Vec<FileChange>,
    seen: &BTreeMap<String, String>,
    reader: &mut Reader,
) -> Vec<FileChange> {
    files
        .into_iter()
        .filter_map(|mut change| {
            let Some(sent) = seen.get(&change.path) else {
                return Some(change);
            };
            let Ok(now) = reader.content_id(&change) else {
                return Some(change);
            };
            if content_key(now) == *sent {
                return None;
            }
            change.old = gix::ObjectId::from_hex(sent.as_bytes()).ok();
            change.status = match (change.old, now) {
                (None, _) => Status::Added,
                (_, None) => Status::Deleted,
                _ => Status::Modified,
            };
            Some(change)
        })
        .collect()
}

/// What a review message needs besides the comments.
#[derive(Debug, Clone)]
pub(crate) struct Header<'a> {
    pub(crate) branch: &'a str,
    pub(crate) worktree: &'a Path,
    pub(crate) base: &'a str,
    /// A worker whose result this reviews: its label and id. Its parent
    /// receives the message and forwards it.
    pub(crate) worker: Option<(&'a str, &'a str)>,
}

/// The review as one Markdown message: the review-level comment first,
/// then numbered comments in file order, each with its place, a short
/// excerpt of the anchored code and its text.
pub(crate) fn message(header: &Header<'_>, comments: &[&Comment]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## Review of `{}` ({}) against {}",
        header.branch,
        header.worktree.display(),
        header.base
    );
    if let Some((label, id)) = header.worker {
        let _ = writeln!(
            out,
            "\nThis reviews worker **{label}** (`{id}`). Forward these comments to it with `message`."
        );
    }
    for c in comments.iter().filter(|c| c.place == Place::Review) {
        let _ = writeln!(out, "\n**Overall:** {}", c.text.trim());
    }
    let mut numbered: Vec<&&Comment> = comments
        .iter()
        .filter(|c| c.place != Place::Review)
        .collect();
    numbered.sort_by_key(|c| {
        (
            c.place.path().map(str::to_owned),
            c.place.lines().map_or(0, |l| l.start),
        )
    });
    for (i, c) in numbered.iter().enumerate() {
        let path = c.place.path().unwrap_or_default();
        let place = match c.place.lines() {
            Some(l) => {
                let range = if l.end > l.start {
                    format!("{}-{}", l.start + 1, l.end + 1)
                } else {
                    format!("{}", l.start + 1)
                };
                let side = if l.side == Side::Old {
                    " (old side)"
                } else {
                    ""
                };
                format!("`{path}:{range}`{side}")
            }
            None => format!("`{path}` (whole file)"),
        };
        let mut tags = Vec::new();
        if c.kind != Kind::Comment {
            tags.push(c.kind.label());
        }
        if c.suggestion.is_some() {
            tags.push("suggestion");
        }
        if c.outdated {
            tags.push("outdated: the lines changed since");
        }
        let tags = if tags.is_empty() {
            String::new()
        } else {
            format!(" · {}", tags.join(" · "))
        };
        let _ = writeln!(out, "\n{}. {place}{tags}", i + 1);
        let lang = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if let Some(l) = c.place.lines() {
            let _ = writeln!(out, "   ```{lang}");
            for line in l.text.iter().take(EXCERPT_LINES) {
                let cut: String = line.chars().take(EXCERPT_WIDTH).collect();
                let _ = writeln!(out, "   {cut}");
            }
            if l.text.len() > EXCERPT_LINES {
                let _ = writeln!(out, "   … {} more lines", l.text.len() - EXCERPT_LINES);
            }
            let _ = writeln!(out, "   ```");
        }
        if let Some(code) = &c.suggestion {
            let _ = writeln!(out, "   Suggested replacement:");
            let _ = writeln!(out, "   ```suggestion");
            for line in code.lines() {
                let _ = writeln!(out, "   {line}");
            }
            let _ = writeln!(out, "   ```");
        }
        for line in c.text.trim().lines() {
            let _ = writeln!(out, "   {line}");
        }
    }
    out.push_str(
        "\nAddress the comments, then reply with which ones you changed and which you did not, and why.\n",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(side: Side, start: u32, text: &[&str]) -> Lines {
        Lines {
            side,
            start,
            end: start + u32::try_from(text.len()).expect("small") - 1,
            text: text.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn anchors_follow_their_text_to_the_nearest_match() {
        let anchored = lines(Side::New, 3, &["b", "c"]);
        // Two lines inserted above: found two further down.
        assert_eq!(
            reanchor(&anchored, &["x", "y", "a", "z", "z", "b", "c"]),
            Some(5)
        );
        // Nearest of two matches wins.
        assert_eq!(
            reanchor(&anchored, &["b", "c", "a", "z", "b", "c"]),
            Some(4)
        );
        assert_eq!(reanchor(&anchored, &["b", "c", "a"]), Some(0));
        // Edited text is gone.
        assert_eq!(reanchor(&anchored, &["a", "b", "C", "d"]), None);
        assert_eq!(reanchor(&anchored, &[]), None);
    }

    #[test]
    fn changed_files_move_drafts_and_outdate_or_drop_the_rest() {
        let mut state = State::default();
        let place = |start, text: &[&str]| Place::Lines {
            path: "a.rs".into(),
            lines: lines(Side::New, start, text),
        };
        let moved = state.add(place(1, &["b"]), Kind::Issue, "x".into(), None);
        let lost = state.add(place(2, &["c"]), Kind::Comment, "y".into(), None);
        let gone = state.add(place(0, &["a"]), Kind::Comment, "z".into(), None);
        let file = state.add(
            Place::File {
                path: "a.rs".into(),
            },
            Kind::Nit,
            "w".into(),
            None,
        );
        state.file_changed("a.rs", Side::New, "v1", &["a", "b", "c"]);
        state.sent(&[gone], BTreeMap::new());
        state.file_changed("a.rs", Side::New, "v2", &["new", "a", "b"]);
        let find = |id| state.comments.iter().find(|c| c.id == id);
        let m = find(moved).expect("kept");
        assert_eq!(
            (m.place.lines().map(|l| l.start), m.outdated),
            (Some(2), false)
        );
        assert!(find(lost).expect("kept").outdated);
        // Sent comments go once their file changes.
        assert!(find(gone).is_none());
        assert!(!find(file).expect("kept").outdated);
    }

    #[test]
    fn marks_clear_when_the_content_changes() {
        let mut state = State::default();
        let (v1, v2) = ("v1", "v2");
        assert!(state.toggle_reviewed("a.rs", v1));
        assert!(state.reviewed("a.rs", v1));
        assert!(!state.reviewed("a.rs", v2));
        assert!(state.toggle_reviewed("a.rs", v2));
        assert_eq!(state.marks.len(), 1);
        assert!(!state.toggle_reviewed("a.rs", v2));
        assert_eq!(state.marks, []);
    }

    #[test]
    fn sending_turns_drafts_sent_and_drops_the_previous_round() {
        let mut state = State::default();
        let a = state.add(Place::Review, Kind::Comment, "a".into(), None);
        state.sent(&[a], BTreeMap::from([("x".into(), "1".into())]));
        let b = state.add(Place::Review, Kind::Comment, "b".into(), None);
        assert_eq!(state.drafts().map(|c| c.id).collect::<Vec<_>>(), vec![b]);
        state.sent(&[b], BTreeMap::new());
        assert_eq!(
            state
                .comments
                .iter()
                .map(|c| (c.id, c.sent))
                .collect::<Vec<_>>(),
            vec![(b, true)]
        );
        assert!(state.seen.is_empty());
    }

    #[test]
    fn state_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("dots-diff-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(State::load(&dir).expect("fresh"), State::default());
        let mut state = State::default();
        state.add(
            Place::Lines {
                path: "a.rs".into(),
                lines: lines(Side::Old, 4, &["x"]),
            },
            Kind::Question,
            "why".into(),
            Some("y".into()),
        );
        state.agent = Some("s-1".into());
        state.save(&dir).expect("save");
        assert_eq!(State::load(&dir).expect("load"), state);
        std::fs::write(dir.join("dots-diff/state.json"), r#"{"version":9}"#).expect("write");
        assert!(State::load(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn messages_number_comments_in_file_order_after_the_overall_one() {
        let mut state = State::default();
        state.add(
            Place::Lines {
                path: "src/b.rs".into(),
                lines: lines(Side::New, 9, &["let x = 1;", "let y = 2;"]),
            },
            Kind::Issue,
            "Use a constant.".into(),
            None,
        );
        state.add(
            Place::Review,
            Kind::Comment,
            "Close, two fixes.".into(),
            None,
        );
        state.add(
            Place::File {
                path: "src/a.rs".into(),
            },
            Kind::Question,
            "Needed?".into(),
            None,
        );
        state.add(
            Place::Lines {
                path: "src/b.rs".into(),
                lines: lines(Side::Old, 2, &["old();"]),
            },
            Kind::Comment,
            String::new(),
            Some("new();".into()),
        );
        let comments: Vec<&Comment> = state.comments.iter().collect();
        let header = Header {
            branch: "feature",
            worktree: Path::new("/w/feature"),
            base: "main 1a2b3c4",
            worker: Some(("fix-auth", "w-1")),
        };
        let text = message(&header, &comments);
        let expected = "## Review of `feature` (/w/feature) against main 1a2b3c4

This reviews worker **fix-auth** (`w-1`). Forward these comments to it with `message`.

**Overall:** Close, two fixes.

1. `src/a.rs` (whole file) · question
   Needed?

2. `src/b.rs:3` (old side) · suggestion
   ```rs
   old();
   ```
   Suggested replacement:
   ```suggestion
   new();
   ```

3. `src/b.rs:10-11` · issue
   ```rs
   let x = 1;
   let y = 2;
   ```
   Use a constant.

Address the comments, then reply with which ones you changed and which you did not, and why.
";
        assert_eq!(text, expected);
    }
}
