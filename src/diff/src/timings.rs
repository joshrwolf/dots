//! `--timings`: when things happened and how long the background work took.

use std::fmt::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};

#[derive(Debug)]
pub struct Timings {
    enabled: bool,
    start: Instant,
    events: Vec<String>,
    diffs: Stats,
    highlights: Stats,
}

#[derive(Debug, Default)]
struct Stats {
    count: u32,
    total: Duration,
    max: Duration,
    lines: u64,
}

impl Stats {
    fn add(&mut self, d: Duration, lines: u32) {
        self.count += 1;
        self.total += d;
        self.max = self.max.max(d);
        self.lines += u64::from(lines);
    }

    fn line(&self, what: &str) -> String {
        if self.count == 0 {
            return format!("{what}: none");
        }
        format!(
            "{what}: {} files, total {:.1?}, mean {:.2?}, max {:.1?}, {} lines",
            self.count,
            self.total,
            self.total / self.count,
            self.max,
            self.lines
        )
    }
}

impl Timings {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            start: Instant::now(),
            events: Vec::new(),
            diffs: Stats::default(),
            highlights: Stats::default(),
        }
    }

    pub fn since_start(&self) -> Duration {
        self.start.elapsed()
    }

    /// Something that took `took`, ending now.
    pub fn span(&mut self, what: &str, took: Duration, detail: String) {
        if self.enabled {
            let at = self.start.elapsed();
            let detail = if detail.is_empty() { String::new() } else { format!(" ({detail})") };
            self.events.push(format!("{at:>10.1?}  {what}: {took:.1?}{detail}"));
        }
    }

    pub fn note(&mut self, what: String) {
        if self.enabled {
            let at = self.start.elapsed();
            self.events.push(format!("{at:>10.1?}  {what}"));
        }
    }

    pub fn diff(&mut self, took: Duration, lines: u32) {
        self.diffs.add(took, lines);
    }

    pub fn highlight(&mut self, took: Duration) {
        self.highlights.add(took, 0);
    }

    pub fn report(&self) -> String {
        let mut out = String::from("dots-diff timings\n");
        for e in &self.events {
            let _ = writeln!(out, "{e}");
        }
        let _ = writeln!(out, "{}", self.diffs.line("diffs"));
        let _ = writeln!(out, "{}", self.highlights.line("highlights"));
        out
    }

    /// Prints the report to `dest`, or standard error.
    pub fn write(&self, _started: Instant, dest: Option<&Path>) -> Result<()> {
        match dest {
            Some(path) => std::fs::write(path, self.report()).with_context(|| format!("writing {}", path.display())),
            None => {
                eprint!("{}", self.report());
                Ok(())
            }
        }
    }
}
