//! Which agent changed which file, from the agents' Pi session files: the
//! paths of their successful edit and write tool calls, and of files a shell
//! command plainly writes (a redirect, `tee`, `touch`, `rm`, `mv`, `cp`,
//! `sed -i`, `git mv`, `git rm`). Code that writes files any other way, such
//! as a script, is not attributed: a file no session names has no author.
//!
//! Session files only grow, so each is read from where the last read
//! stopped, a bounded amount at a time.

use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::time::SystemTime;

use serde::Deserialize;

/// The most of one session file read per pass; the rest waits for the next.
const MAX_READ: u64 = 64 << 20;

/// An agent and the session file it records its work in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    pub(crate) name: String,
    pub(crate) file: PathBuf,
}

/// The authors of a worktree's files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Attribution {
    pub(crate) worktree: PathBuf,
    /// Agents whose sessions were read, in the order given.
    pub(crate) authors: Vec<String>,
    /// Per path within the worktree, the indexes of its authors.
    pub(crate) files: HashMap<String, Vec<usize>>,
    /// Session files that could not be read.
    pub(crate) problems: Vec<String>,
}

impl Attribution {
    /// The authors of `path`, a path within the worktree.
    pub(crate) fn authors_of<'a>(&'a self, path: &str) -> impl Iterator<Item = &'a str> + use<'a> {
        self.files
            .get(path)
            .into_iter()
            .flatten()
            .map(|&i| self.authors[i].as_str())
    }
}

/// What one session file has shown so far.
#[derive(Debug, Default)]
struct Parsed {
    len: u64,
    modified: Option<SystemTime>,
    /// Where the next read starts: after the last complete line.
    offset: u64,
    /// The session's directory, which relative paths are in.
    cwd: Option<PathBuf>,
    /// Paths of tool calls that have no result yet, by call id.
    pending: HashMap<String, Vec<PathBuf>>,
    /// Absolute paths the session wrote.
    written: BTreeSet<PathBuf>,
}

/// Session files read so far, kept between attributions.
#[derive(Debug, Default)]
pub(crate) struct Sessions {
    parsed: HashMap<PathBuf, Parsed>,
    /// The last attribution stopped short of the end of a session file.
    behind: bool,
}

impl Sessions {
    /// Attributes the files of `worktree`, a canonical path, to `sources`.
    pub(crate) fn attribute(&mut self, worktree: &Path, sources: &[Source]) -> Attribution {
        let mut out = Attribution {
            worktree: worktree.to_owned(),
            ..Attribution::default()
        };
        let mut parents: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
        self.behind = false;
        for source in sources {
            let parsed = match self.read(&source.file) {
                Ok(p) => p,
                Err(e) => {
                    out.problems.push(format!("{}: {e}", source.file.display()));
                    continue;
                }
            };
            let author = out.authors.len();
            out.authors.push(source.name.clone());
            for path in &parsed.written {
                let Some(rel) = within(path, worktree, &mut parents) else {
                    continue;
                };
                let authors = out.files.entry(rel).or_default();
                if !authors.contains(&author) {
                    authors.push(author);
                }
            }
        }
        out
    }

    /// Reads what `file` gained since the last read.
    fn read(&mut self, file: &Path) -> std::io::Result<&Parsed> {
        let meta = std::fs::metadata(file)?;
        let (len, modified) = (meta.len(), meta.modified().ok());
        let parsed = self.parsed.entry(file.to_owned()).or_default();
        if len < parsed.offset {
            // Rewritten, not appended to: start over.
            *parsed = Parsed::default();
        }
        if (len, modified) == (parsed.len, parsed.modified) {
            return Ok(parsed);
        }
        let mut f = File::open(file)?;
        f.seek(SeekFrom::Start(parsed.offset))?;
        let mut bytes = Vec::new();
        f.take(MAX_READ).read_to_end(&mut bytes)?;
        let complete = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        for line in bytes[..complete].split(|&b| b == b'\n') {
            parsed.line(line);
        }
        parsed.offset += complete as u64;
        // Unchanged once everything up to the end is read.
        if parsed.offset == len {
            (parsed.len, parsed.modified) = (len, modified);
        } else if complete > 0 {
            self.behind = true;
        }
        Ok(parsed)
    }
}

/// `path` relative to `worktree`, also through a symlinked directory.
fn within(
    path: &Path,
    worktree: &Path,
    parents: &mut HashMap<PathBuf, Option<PathBuf>>,
) -> Option<String> {
    if let Ok(rel) = path.strip_prefix(worktree) {
        return Some(rel.to_string_lossy().into_owned());
    }
    let parent = path.parent()?;
    let real = parents
        .entry(parent.to_owned())
        .or_insert_with(|| parent.canonicalize().ok())
        .as_ref()?;
    let full = real.join(path.file_name()?);
    Some(
        full.strip_prefix(worktree)
            .ok()?
            .to_string_lossy()
            .into_owned(),
    )
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Entry {
    Session {
        cwd: PathBuf,
    },
    Message {
        message: Message,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "role", rename_all = "camelCase")]
enum Message {
    Assistant {
        #[serde(default)]
        content: Vec<Part>,
    },
    #[serde(rename_all = "camelCase")]
    ToolResult {
        tool_call_id: String,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Part {
    ToolCall {
        id: String,
        name: String,
        #[serde(default)]
        arguments: Arguments,
    },
    #[serde(other)]
    Other,
}

#[derive(Default, Deserialize)]
struct Arguments {
    path: Option<String>,
    command: Option<String>,
}

impl Parsed {
    fn line(&mut self, line: &[u8]) {
        // Most lines are text; only these three shapes matter.
        let wanted = [
            &b"\"toolCall\""[..],
            b"\"toolResult\"",
            b"\"type\":\"session\"",
        ];
        if !wanted.iter().any(|w| contains(line, w)) {
            return;
        }
        let Ok(entry) = serde_json::from_slice::<Entry>(line) else {
            return;
        };
        match entry {
            Entry::Session { cwd } => self.cwd = Some(cwd.canonicalize().unwrap_or(cwd)),
            Entry::Message {
                message: Message::Assistant { content },
            } => {
                for part in content {
                    let Part::ToolCall {
                        id,
                        name,
                        arguments,
                    } = part
                    else {
                        continue;
                    };
                    let paths = match (name.as_str(), arguments) {
                        ("edit" | "write", Arguments { path: Some(p), .. }) => {
                            self.resolve(Path::new(""), &p).into_iter().collect()
                        }
                        (
                            "bash",
                            Arguments {
                                command: Some(c), ..
                            },
                        ) => written_by(&c)
                            .into_iter()
                            .filter_map(|(dir, p)| self.resolve(&dir, &p))
                            .collect(),
                        _ => Vec::new(),
                    };
                    if !paths.is_empty() {
                        self.pending.insert(id, paths);
                    }
                }
            }
            Entry::Message {
                message:
                    Message::ToolResult {
                        tool_call_id,
                        is_error,
                    },
            } => {
                if let Some(paths) = self.pending.remove(&tool_call_id)
                    && !is_error
                {
                    self.written.extend(paths);
                }
            }
            Entry::Message { .. } | Entry::Other => {}
        }
    }

    /// The absolute path `path` names from `dir`, itself relative to the
    /// session's directory; `None` when that is unknown.
    fn resolve(&self, dir: &Path, path: &str) -> Option<PathBuf> {
        let path = Path::new(path);
        let full = if path.is_absolute() {
            path.to_owned()
        } else {
            self.cwd.as_ref()?.join(dir).join(path)
        };
        Some(normalize(&full))
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// `path` without `.` and `..` components.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// Files a shell command plainly writes or removes, each with the
/// directory it is named from (after a `cd`), relative to where the command
/// starts. Anything with expansions or globs is skipped, never guessed.
fn written_by(command: &str) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut dir = PathBuf::new();
    for words in simple_commands(command) {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let mut named: Vec<&str> = Vec::new();
        // Redirects of standard output, in any command.
        let mut args: Vec<&str> = Vec::new();
        let mut it = words.iter().copied();
        while let Some(w) = it.next() {
            match w {
                ">" | ">>" | ">|" => named.extend(it.next()),
                _ if w.starts_with(">>") => named.push(&w[2..]),
                _ if w.starts_with('>') => named.push(&w[1..]),
                _ if w.starts_with("<<") || w.starts_with('<') => {
                    if w == "<" || w == "<<" || w == "<<-" {
                        it.next();
                    }
                }
                _ => args.push(w),
            }
        }
        match args.as_slice() {
            ["cd", to] => {
                dir = dir.join(to);
                continue;
            }
            ["tee" | "touch" | "rm" | "mv", rest @ ..] | ["git", "mv" | "rm", rest @ ..] => {
                named.extend(operands(rest));
            }
            ["cp", rest @ ..] => named.extend(operands(rest).last()),
            ["sed", rest @ ..] if rest.iter().any(|a| a.starts_with("-i")) => {
                let mut files = Vec::new();
                let mut scripted = false;
                let mut words = rest.iter().copied().peekable();
                while let Some(w) = words.next() {
                    match w {
                        "-e" | "-f" => {
                            scripted = true;
                            words.next();
                        }
                        // macOS sed takes a bare -i's backup suffix as a word.
                        "-i" if words.peek() == Some(&"") => {
                            words.next();
                        }
                        w if !w.starts_with('-') => files.push(w),
                        _ => {}
                    }
                }
                // Without -e or -f, the first operand is the script.
                named.extend(files.into_iter().skip(usize::from(!scripted)));
            }
            _ => {}
        }
        for name in named {
            if !name.is_empty() && !name.contains(['$', '*', '?', '[', '`', '~']) {
                out.push((dir.clone(), name.to_owned()));
            }
        }
    }
    out
}

/// Words that are not options.
fn operands<'a>(words: &[&'a str]) -> Vec<&'a str> {
    words
        .iter()
        .copied()
        .filter(|a| !a.starts_with('-'))
        .collect()
}

/// A command line's simple commands as words, quotes removed, skipping
/// here-document bodies.
fn simple_commands(command: &str) -> Vec<Vec<String>> {
    let mut commands = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word: Option<String> = None;
    let mut heredocs: Vec<String> = Vec::new();
    let mut chars = command.chars().peekable();
    let end_word = |word: &mut Option<String>, words: &mut Vec<String>| {
        if let Some(w) = word.take() {
            words.push(w);
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                let w = word.get_or_insert_with(String::new);
                for c in chars.by_ref() {
                    if c == '\'' {
                        break;
                    }
                    w.push(c);
                }
            }
            '"' => {
                let w = word.get_or_insert_with(String::new);
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => w.extend(chars.next()),
                        c => w.push(c),
                    }
                }
            }
            '\\' => {
                if let Some(c) = chars.next()
                    && c != '\n'
                {
                    word.get_or_insert_with(String::new).push(c);
                }
            }
            ' ' | '\t' => end_word(&mut word, &mut words),
            '\n' | ';' | '&' | '|' | '(' | ')' => {
                end_word(&mut word, &mut words);
                // `<<WORD` or `<<'WORD'`: its body follows the line.
                let mut ws = words.iter();
                while let Some(w) = ws.next() {
                    if let Some(tag) = w.strip_prefix("<<") {
                        let tag = match tag.trim_start_matches('-') {
                            "" => ws.next().map_or("", String::as_str),
                            tag => tag,
                        };
                        heredocs.push(tag.to_owned());
                    }
                }
                if !words.is_empty() {
                    commands.push(std::mem::take(&mut words));
                }
                if c == '\n' {
                    for tag in std::mem::take(&mut heredocs) {
                        // Skip the body up to the line holding only the tag.
                        loop {
                            let line: String = chars.by_ref().take_while(|&c| c != '\n').collect();
                            if line.trim() == tag || chars.peek().is_none() {
                                break;
                            }
                        }
                    }
                }
            }
            '>' => {
                // A redirect is a word of its own, joined to its target.
                end_word(&mut word, &mut words);
                let mut op = String::from(">");
                while let Some(&c) = chars.peek() {
                    if c == '>' || c == '|' {
                        op.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                words.push(op);
            }
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    end_word(&mut word, &mut words);
    if !words.is_empty() {
        commands.push(words);
    }
    commands
}

/// A request for one worktree's attribution.
#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) worktree: PathBuf,
    pub(crate) sources: Vec<Source>,
}

/// Attributes on a thread of its own, keeping what it read between
/// requests; only the latest of several waiting requests is answered.
pub(crate) fn spawn(done: impl Fn(Attribution) -> bool + Send + 'static) -> Sender<Request> {
    let (tx, rx): (Sender<Request>, Receiver<Request>) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut sessions = Sessions::default();
        let mut waiting = rx.recv().ok();
        while let Some(mut request) = waiting.take() {
            while let Ok(newer) = rx.try_recv() {
                request = newer;
            }
            if !done(sessions.attribute(&request.worktree, &request.sources)) {
                return;
            }
            // Large files are read a bounded part at a time, each answered.
            waiting = if sessions.behind {
                Some(request)
            } else {
                rx.recv().ok()
            };
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(command: &str) -> Vec<String> {
        written_by(command)
            .into_iter()
            .map(|(dir, p)| dir.join(p).to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn shell_commands_name_the_files_they_plainly_write() {
        assert_eq!(
            named("echo hi > a.txt && rm -f b.txt c.txt"),
            ["a.txt", "b.txt", "c.txt"]
        );
        assert_eq!(named("printf x >>log/out.txt"), ["log/out.txt"]);
        assert_eq!(named("cmd | tee -a 'with space.md'"), ["with space.md"]);
        assert_eq!(named("cp src/a.rs src/b.rs"), ["src/b.rs"]);
        assert_eq!(
            named("mv old.rs new.rs; git rm -q gone.rs"),
            ["old.rs", "new.rs", "gone.rs"]
        );
        assert_eq!(named("sed -i 's/a/b/' x.rs y.rs"), ["x.rs", "y.rs"]);
        assert_eq!(named("sed -i '' -e 's/a/b/' x.rs"), ["x.rs"]);
        assert_eq!(named("cd sub && touch new.rs"), ["sub/new.rs"]);
        assert_eq!(
            named("cat > notes.md <<'EOF'\nrm everything.rs\n> not-a-file\nEOF\ntouch after.rs"),
            ["notes.md", "after.rs"],
            "a here-document's body is text"
        );
    }

    #[test]
    fn shell_commands_are_never_guessed_at() {
        for command in [
            "cargo test 2>&1 | tail -5",
            "rm *.rs",
            "echo x > \"$OUT\"",
            "sed 's/a/b/' x.rs",
            "python3 fix.py src/a.rs",
            "grep -n foo src/a.rs > /dev/null",
            "cat ~/x > ~/y",
        ] {
            let found = named(command);
            assert!(
                found.is_empty() || found == ["/dev/null"],
                "{command}: {found:?}"
            );
        }
    }

    /// A session file in the shape Pi writes, with the given lines after
    /// its header.
    fn session(cwd: &Path, lines: &[String]) -> String {
        let mut out = format!(
            "{}\n",
            serde_json::json!({"type": "session", "version": 3, "id": "s", "cwd": cwd})
        );
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    fn call(id: &str, name: &str, arguments: &serde_json::Value) -> String {
        serde_json::json!({"type": "message", "message": {"role": "assistant", "content": [
            {"type": "text", "text": "editing"},
            {"type": "toolCall", "id": id, "name": name, "arguments": arguments},
        ]}})
        .to_string()
    }

    fn result(id: &str, error: bool) -> String {
        serde_json::json!({"type": "message", "message": {"role": "toolResult", "toolCallId": id,
            "toolName": "edit", "content": [{"type": "text", "text": "ok"}], "isError": error}})
        .to_string()
    }

    #[test]
    fn attributes_files_to_the_sessions_that_wrote_them() {
        let root = std::env::temp_dir().join(format!("dots-diff-attr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let wt = root.join("wt");
        std::fs::create_dir_all(wt.join("src")).expect("dir");
        let wt = wt.canonicalize().expect("canonical");
        let worker = root.join("worker.jsonl");
        let parent = root.join("parent.jsonl");
        std::fs::write(
            &worker,
            session(
                &wt.join("src"),
                &[
                    call(
                        "1",
                        "edit",
                        &serde_json::json!({"path": "a.rs", "edits": []}),
                    ),
                    result("1", false),
                    call("2", "write", &serde_json::json!({"path": "failed.rs"})),
                    result("2", true),
                    call(
                        "3",
                        "bash",
                        &serde_json::json!({"command": "echo x > ../shared.md"}),
                    ),
                    result("3", false),
                    call("4", "edit", &serde_json::json!({"path": "/elsewhere/x.rs"})),
                    result("4", false),
                    call("5", "write", &serde_json::json!({"path": "unfinished.rs"})),
                ],
            ),
        )
        .expect("write");
        std::fs::write(
            &parent,
            session(
                &root,
                &[
                    call(
                        "p1",
                        "write",
                        &serde_json::json!({"path": wt.join("shared.md")}),
                    ),
                    result("p1", false),
                ],
            ),
        )
        .expect("write");
        let sources = [
            Source {
                name: "fix".into(),
                file: worker.clone(),
            },
            Source {
                name: "pi lead".into(),
                file: parent,
            },
            Source {
                name: "gone".into(),
                file: root.join("missing.jsonl"),
            },
        ];
        let mut sessions = Sessions::default();
        let got = sessions.attribute(&wt, &sources);
        assert_eq!(got.authors, ["fix", "pi lead"]);
        let authors = |a: &Attribution, p: &str| -> Vec<String> {
            a.authors_of(p).map(str::to_owned).collect()
        };
        assert_eq!(authors(&got, "src/a.rs"), ["fix"]);
        assert_eq!(authors(&got, "shared.md"), ["fix", "pi lead"]);
        assert_eq!(got.files.len(), 2, "{:?}", got.files);
        assert_eq!(got.problems.len(), 1);
        // The unfinished call succeeds later; only the new lines are read.
        let mut more = std::fs::OpenOptions::new()
            .append(true)
            .open(&worker)
            .expect("open");
        std::io::Write::write_all(
            &mut more,
            format!("{}\n{{\"type\":", result("5", false)).as_bytes(),
        )
        .expect("append");
        let got = sessions.attribute(&wt, &sources);
        assert_eq!(authors(&got, "src/unfinished.rs"), ["fix"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
