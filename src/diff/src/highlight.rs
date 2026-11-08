//! Syntax highlighting with tree-sitter grammars (arborium). Whole old and new
//! files are highlighted, not diff fragments, so every row sees its real
//! context; rows then index the per-line runs. Work runs on background
//! threads in the order the viewer asks for it.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::diff::Text;

/// What a run of source text is, as far as colour goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum Role {
    Plain,
    Keyword,
    String,
    Comment,
    Function,
    Type,
    Constant,
    Operator,
    Punctuation,
    Property,
    Variable,
    Tag,
    Attribute,
    Macro,
}

impl Role {
    pub(crate) const COUNT: usize = 14;
    pub(crate) const ALL: [Role; Self::COUNT] = [
        Role::Plain,
        Role::Keyword,
        Role::String,
        Role::Comment,
        Role::Function,
        Role::Type,
        Role::Constant,
        Role::Operator,
        Role::Punctuation,
        Role::Property,
        Role::Variable,
        Role::Tag,
        Role::Attribute,
        Role::Macro,
    ];

    /// Maps a grammar capture name such as `keyword.return` to a role.
    pub(crate) fn of_capture(capture: &str) -> Option<Role> {
        let capture = capture.strip_prefix('@').unwrap_or(capture);
        let head = capture.split('.').next().unwrap_or(capture);
        Some(match head {
            "keyword" | "include" | "conditional" | "repeat" | "exception" | "storageclass"
            | "preproc" | "define" | "structure" => Role::Keyword,
            "string" | "character" | "escape" => Role::String,
            "comment" => Role::Comment,
            "function" | "method" | "constructor" => {
                if capture == "function.macro" {
                    Role::Macro
                } else {
                    Role::Function
                }
            }
            "type" | "namespace" | "module" => Role::Type,
            "constant" | "boolean" | "number" | "float" => Role::Constant,
            "operator" => Role::Operator,
            "punctuation" => Role::Punctuation,
            "property" | "field" => Role::Property,
            "variable" | "parameter" => Role::Variable,
            "tag" => Role::Tag,
            "attribute" | "label" => Role::Attribute,
            "macro" => Role::Macro,
            "markup" | "text" => match capture {
                "markup.heading" | "text.title" => Role::Keyword,
                "markup.raw" | "markup.raw.block" | "markup.raw.inline" | "text.literal" => {
                    Role::String
                }
                "markup.link.url" | "text.uri" => Role::Function,
                _ => return None,
            },
            _ => return None,
        })
    }
}

/// A coloured byte range within one line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) role: Role,
}

/// Runs for every line of a file; lines without colour have none.
#[derive(Debug, Default)]
pub(crate) struct Lines {
    pub(crate) lines: Vec<Vec<Run>>,
}

impl Lines {
    pub(crate) fn line(&self, i: u32) -> &[Run] {
        self.lines.get(i as usize).map_or(&[], Vec::as_slice)
    }
}

/// Files above either bound are shown without syntax colour.
pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_LINES: u32 = 60_000;

/// The grammar for a path, if any.
pub(crate) fn language(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name {
        "Makefile" | "makefile" | "GNUmakefile" => return Some("make"),
        "Dockerfile" => return Some("dockerfile"),
        "Cargo.lock" | "uv.lock" => return Some("toml"),
        _ => {}
    }
    let lang = arborium::detect_language(path)?;
    // Only grammars compiled in can be used.
    SUPPORTED.contains(&lang).then_some(lang)
}

const SUPPORTED: &[&str] = &[
    "bash",
    "c",
    "cpp",
    "css",
    "dockerfile",
    "go",
    "html",
    "javascript",
    "json",
    "lua",
    "make",
    "markdown",
    "nix",
    "python",
    "rust",
    "sql",
    "toml",
    "tsx",
    "typescript",
    "yaml",
];

/// Colours one whole file. Inner captures win over the ones enclosing them,
/// and later query patterns win ties, as tree-sitter intends.
pub(crate) fn highlight(
    highlighter: &mut arborium::Highlighter,
    language: &str,
    text: &Text,
) -> Result<Lines, String> {
    let src: &str = &text.src;
    let mut spans = highlighter
        .highlight_spans(language, src)
        .map_err(|e| e.to_string())?;
    spans.sort_by(|a, b| {
        (b.end - b.start)
            .cmp(&(a.end - a.start))
            .then(a.pattern_index.cmp(&b.pattern_index))
    });
    let mut roles = vec![Role::Plain; src.len()];
    for span in &spans {
        let Some(role) = Role::of_capture(&span.capture) else {
            continue;
        };
        let end = (span.end as usize).min(roles.len());
        let start = (span.start as usize).min(end);
        roles[start..end].fill(role);
    }
    let mut lines = Vec::with_capacity(text.len() as usize);
    for i in 0..text.len() {
        let start = text.start(i) as usize;
        let len = text.line(i).len();
        let mut runs: Vec<Run> = Vec::new();
        for (offset, &role) in (0u32..).zip(&roles[start..start + len]) {
            match runs.last_mut() {
                Some(run) if run.role == role && run.end == offset => run.end += 1,
                _ => runs.push(Run {
                    start: offset,
                    end: offset + 1,
                    role,
                }),
            }
        }
        runs.retain(|r| r.role != Role::Plain);
        lines.push(runs);
    }
    Ok(Lines { lines })
}

/// Cache key: content hash and grammar.
pub(crate) type Key = (u64, &'static str);

#[derive(Debug, Clone)]
pub(crate) struct Job {
    pub(crate) key: Key,
    pub(crate) text: Arc<Text>,
}

/// A finished job: the runs, or why there are none, and how long it took.
#[derive(Debug)]
pub(crate) struct Done {
    pub(crate) key: Key,
    pub(crate) result: Result<Arc<Lines>, String>,
    pub(crate) elapsed: Duration,
}

#[derive(Debug, Default)]
struct Queue {
    jobs: VecDeque<Job>,
    closed: bool,
}

/// Background highlighting threads sharing one queue and grammar store.
pub(crate) struct Pool {
    queue: Arc<(Mutex<Queue>, Condvar)>,
    in_flight: Arc<Mutex<HashSet<Key>>>,
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pool").finish_non_exhaustive()
    }
}

impl Pool {
    pub(crate) fn new(threads: usize, done: impl Fn(Done) + Send + Sync + 'static) -> Self {
        let queue = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let in_flight = Arc::new(Mutex::new(HashSet::new()));
        let done = Arc::new(done);
        let base = arborium::Highlighter::new();
        for _ in 0..threads {
            let (queue, in_flight, done) = (queue.clone(), in_flight.clone(), done.clone());
            let mut highlighter = base.fork();
            std::thread::spawn(move || {
                while let Some(job) = next(&queue) {
                    let started = Instant::now();
                    let result = highlight(&mut highlighter, job.key.1, &job.text).map(Arc::new);
                    in_flight.lock().expect("highlight state").remove(&job.key);
                    done(Done {
                        key: job.key,
                        result,
                        elapsed: started.elapsed(),
                    });
                }
            });
        }
        Self { queue, in_flight }
    }

    /// Replaces the pending work with `jobs`, most wanted first. Jobs already
    /// running are not repeated.
    pub(crate) fn want(&self, jobs: Vec<Job>) {
        let (lock, wake) = &*self.queue;
        let mut queue = lock.lock().expect("highlight queue");
        let mut in_flight = self.in_flight.lock().expect("highlight state");
        // Jobs dropped from the queue are no longer in flight.
        for job in &queue.jobs {
            in_flight.remove(&job.key);
        }
        queue.jobs.clear();
        for job in jobs {
            if in_flight.insert(job.key) {
                queue.jobs.push_back(job);
            }
        }
        wake.notify_all();
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        let (lock, wake) = &*self.queue;
        if let Ok(mut queue) = lock.lock() {
            queue.closed = true;
            queue.jobs.clear();
        }
        wake.notify_all();
    }
}

fn next(queue: &(Mutex<Queue>, Condvar)) -> Option<Job> {
    let (lock, wake) = queue;
    let mut queue = lock.lock().ok()?;
    loop {
        if queue.closed {
            return None;
        }
        if let Some(job) = queue.jobs.pop_front() {
            return Some(job);
        }
        queue = wake.wait(queue).ok()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_capture_names_to_roles() {
        assert_eq!(Role::of_capture("keyword.return"), Some(Role::Keyword));
        assert_eq!(Role::of_capture("@string.escape"), Some(Role::String));
        assert_eq!(Role::of_capture("function.macro"), Some(Role::Macro));
        assert_eq!(Role::of_capture("spell"), None);
    }

    #[test]
    fn highlights_rust_into_per_line_runs() {
        let mut hl = arborium::Highlighter::new();
        let text = Text::new(Arc::from("fn main() {\n    // hi\n}\n"));
        let lines = highlight(&mut hl, "rust", &text).expect("rust grammar");
        assert_eq!(lines.lines.len(), 3);
        assert_eq!(
            lines.line(0).first(),
            Some(&Run {
                start: 0,
                end: 2,
                role: Role::Keyword
            })
        );
        assert_eq!(
            lines.line(1),
            &[Run {
                start: 4,
                end: 9,
                role: Role::Comment
            }]
        );
    }
}
