//! Bounded navigation over a local log spool. No provider calls happen here.
use std::fs::File;
use std::io::{BufRead as _, Seek as _, SeekFrom};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{CheckRef, LogMatch, LogPage, LogSource, Result};

pub(crate) struct Navigation {
    pub command: String,
    pub full_command: String,
    pub supports_tests: bool,
    pub failed_steps: Vec<(String, String)>,
}
impl Navigation {
    fn in_failed_step(&self, line: &str) -> bool {
        self.failed_steps.iter().any(|(start, end)| {
            let Some(timestamp) = line.get(..19) else {
                return false;
            };
            start
                .get(..19)
                .zip(end.get(..19))
                .is_some_and(|(start, end)| start <= end && timestamp >= start && timestamp <= end)
        })
    }

    pub(crate) fn job(reference: &CheckRef) -> Self {
        Self {
            failed_steps: Vec::new(),
            command: reference.command(),
            supports_tests: true,
            full_command: format!(
                "gh run view --repo {}/{}/{} --job {} --log",
                reference.repository.host,
                reference.repository.owner,
                reference.repository.repo,
                reference.job
            ),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Query<'a> {
    Window,
    Search(&'a str),
    Test(&'a str),
}

#[derive(Default)]
struct Scan {
    total: usize,
    matches: usize,
    eligible: usize,
    hits: Vec<LogMatch>,
    next: Option<usize>,
    anchor: Option<usize>,
    rank: u8,
    dependency: bool,
    failed_step_first: Option<usize>,
    failed_step_last: Option<usize>,
}

impl Scan {
    fn prefer_failed_step(&mut self, step: FailedStep, count: usize) {
        if self.rank < 8
            && let Some(last) = self.failed_step_last
        {
            // No recognized diagnostic in the failed step: show its tail, not cleanup.
            self.anchor = Some(
                last.saturating_sub(count.saturating_sub(4))
                    .max(self.failed_step_first.unwrap_or(1)),
            );
            self.hits.clear();
            self.matches = 0;
            self.eligible = 0;
        }
        if self.rank < 5
            && self.failed_step_last.is_none()
            && let Some(hit) = step.failed
        {
            self.anchor = Some(hit.line);
            self.hits = vec![hit];
            self.matches = 1;
            self.eligible = 1;
        }
    }
}

pub(crate) fn page(
    file: &mut File,
    reference: &Navigation,
    start: Option<usize>,
    count: usize,
    source: LogSource,
    fetched_at: u64,
    query: Query<'_>,
) -> Result<LogPage> {
    let origin = file.stream_position()?;
    let scan = scan(file, reference, start.unwrap_or(1), count, query)?;
    let mut anchor = scan.anchor;
    if matches!(query, Query::Window) && start.is_none() && reference.supports_tests {
        file.seek(SeekFrom::Start(origin))?;
        anchor = assertion_anchor(file, &scan.hits)?.or(anchor);
    }
    if let Query::Test(test) = query
        && start.is_none()
    {
        file.seek(SeekFrom::Start(origin))?;
        anchor = assertion_for(file, &[test.to_owned()])?.or(anchor);
    }
    let window_start = if matches!(query, Query::Search(_)) {
        Some(start.unwrap_or(1))
    } else {
        start
    };
    let offset = window_start
        .map_or_else(
            || {
                anchor.map_or(
                    if reference.supports_tests {
                        scan.total.saturating_sub(count)
                    } else {
                        0
                    },
                    |line| line.saturating_sub(1 + (count / 3).min(3)),
                )
            },
            |line| line - 1,
        )
        .min(scan.total);
    file.seek(SeekFrom::Start(origin))?;
    let (lines, numbers, shortened, byte_limit, next) = match query {
        Query::Search(_) => (Vec::new(), Vec::new(), 0, false, None),
        Query::Window => excerpt(file, offset, count, None)?,
        Query::Test(test) => excerpt(
            file,
            if start.is_none() {
                anchor.unwrap_or(1) - 1
            } else {
                offset
            },
            count,
            Some(test),
        )?,
    };
    let command = reference.command.clone();
    let test = match query {
        Query::Test(test) => Some(test),
        _ => None,
    };
    let search = match query {
        Query::Search(search) => Some(search),
        _ => None,
    };
    let start_line = numbers.first().copied().unwrap_or(offset + 1);
    let end = numbers.last().copied().unwrap_or(offset);
    Ok(LogPage {
        source, fetched_at, start_line, total_lines: scan.total, lines, line_numbers: numbers,
        test: test.map(str::to_owned), search: search.map(str::to_owned),
        navigation_note: match query {
            Query::Search(_) if scan.matches == 0 => "No matching lines.",
            Query::Search(_) if scan.eligible == 0 => "No matches at or after this cursor.",
            Query::Search(_) => "Compact match index; follow Context for surrounding lines.",
            Query::Test(_) => "Go test attribution follows RUN/NAME/CONT markers. Concurrent package output can be ambiguous; original line numbers are preserved.",
            Query::Window if !reference.supports_tests && start.is_none() && anchor.is_some() => "Heuristic window near a check-output diagnostic.",
            Query::Window if !reference.supports_tests => "Window in GitHub check-run title, summary and text.",
            Query::Window if start.is_none() && scan.failed_step_last.is_some() && scan.rank < 8 => "Failed-step tail fallback: no recognized diagnostic was found within its timestamps.",
            Query::Window if start.is_none() && scan.failed_step_last.is_some() => "Heuristic window within a failed step's timestamps; verify the surrounding evidence.",
            Query::Window if start.is_none() && anchor.is_none() => "Tail fallback: no recognized failure diagnostic was found.",
            Query::Window if start.is_none() => "Heuristic navigation: structured failures outrank generic errors; attributed Go diagnostics are preferred when available.",
            Query::Window if reference.supports_tests => "Explicit window in the original job log.",
            Query::Window => "Window in GitHub check-run title, summary and text.",
        }.into(),
        dependency_marker: scan.dependency,
        omitted_before: start_line - 1, omitted_after: scan.total.saturating_sub(end),
        next_command: next.map(|line| match test {
            Some(test) => format!("{command} --test {} --start {line} --lines {count}", quote(test)),
            None => format!("{command} --start {line} --lines {count}"),
        }),
        previous_command: (search.is_none() && test.is_none() && offset > 0).then(||
            format!("{command} --start {} --lines {count}", offset.saturating_sub(count) + 1)),
        matches: scan.matches, eligible_matches: scan.eligible, match_index: scan.hits,
        next_match_command: search.and_then(|search| scan.next.map(|line|
            format!("{command} --search {} --start {line} --lines {count}", quote(search)))),
        byte_limit_reached: byte_limit, shortened_lines: shortened,
        full_log_command: reference.full_command.clone(),
    })
}

fn scan(
    file: &mut File,
    reference: &Navigation,
    start: usize,
    count: usize,
    query: Query<'_>,
) -> Result<Scan> {
    let mut scan = Scan::default();
    let mut step = FailedStep::default();
    for (index, line) in log_lines(file).enumerate() {
        let line = line?;
        let number = index + 1;
        scan.total = number;
        let text = payload(&line);
        if matches!(query, Query::Window) && reference.supports_tests {
            step.observe(text, number, &line, reference, count);
        }
        let in_failed_step = matches!(query, Query::Window) && reference.in_failed_step(&line);
        if in_failed_step {
            scan.failed_step_first.get_or_insert(number);
            scan.failed_step_last = Some(number);
        }
        let lower = text.to_ascii_lowercase();
        scan.dependency |= (lower.starts_with("needs results:") && lower.contains("failure"))
            || [
                "dependency failed",
                "dependencies failed",
                "dependent jobs failed",
                "required job failed",
                "dependency failure",
            ]
            .iter()
            .any(|marker| lower.starts_with(marker));
        let is_match = match query {
            Query::Search(needle) => line.contains(needle),
            Query::Test(_) => false,
            Query::Window => {
                let mut rank = if reference.supports_tests {
                    failure_rank(text)
                } else {
                    output_rank(text)
                };
                if in_failed_step && rank > 0 {
                    rank += 8;
                }
                if rank > scan.rank {
                    scan.rank = rank;
                    scan.hits.clear();
                    scan.anchor = None;
                    scan.matches = 0;
                    scan.eligible = 0;
                }
                rank > 0 && rank == scan.rank
            }
        };
        if !is_match {
            continue;
        }
        scan.matches += 1;
        if number < start {
            continue;
        }
        scan.eligible += 1;
        scan.anchor.get_or_insert(number);
        if scan.hits.len() < 10 {
            let (preview, shortened) = shorten(&line, 512);
            let name = go_event(text)
                .filter(|(event, _)| *event == "FAIL")
                .map(|(_, name)| name);
            scan.hits.push(LogMatch {
                line: number,
                preview,
                preview_shortened: shortened,
                test_name: name.map(str::to_owned),
                command: format!(
                    "{} --start {} --lines {count}",
                    reference.command,
                    context_start(number, count)
                ),
                test_output_command: name.filter(|_| reference.supports_tests).map(|name| {
                    format!(
                        "{} --test {} --lines {count}",
                        reference.command.clone(),
                        quote(name)
                    )
                }),
            });
        } else if scan.next.is_none() {
            scan.next = Some(number);
        }
    }
    scan.prefer_failed_step(step, count);
    Ok(scan)
}

// Three bounded lines plus the matched line fit comfortably inside the byte budget.
fn context_start(number: usize, count: usize) -> usize {
    number.saturating_sub(count.saturating_sub(1).min(3)).max(1)
}

fn output_rank(text: &str) -> u8 {
    if text.starts_with("error[E") && text.contains("]:") {
        return 4;
    }
    if let Some((path, rest)) = text.split_once(':')
        && !path.contains(char::is_whitespace)
        && std::path::Path::new(path).extension().is_some()
        && let Some((line, rest)) = rest.split_once(':')
        && !line.is_empty()
        && line.bytes().all(|b| b.is_ascii_digit())
    {
        let message = rest
            .split_once(':')
            .filter(|(column, _)| !column.is_empty() && column.bytes().all(|b| b.is_ascii_digit()))
            .map_or(rest, |(_, message)| message)
            .trim_start();
        if [
            "undefined:",
            "error:",
            "fatal error:",
            "cannot ",
            "could not ",
            "syntax error",
            "invalid operation:",
            "not enough arguments",
            "too many arguments",
        ]
        .iter()
        .any(|prefix| message.starts_with(prefix))
        {
            return 4;
        }
    }
    if text.starts_with("Error:") || text.starts_with("error:") {
        return 2;
    }
    failure_rank(text)
}

#[derive(Default)]
struct FailedStep {
    candidate: Option<(u8, LogMatch)>,
    failed: Option<LogMatch>,
}
impl FailedStep {
    fn observe(
        &mut self,
        text: &str,
        number: usize,
        line: &str,
        reference: &Navigation,
        count: usize,
    ) {
        if text.starts_with("##[group]Run ") {
            self.candidate = None;
        }
        let rank = failure_rank(text);
        if rank == 1 {
            if self.failed.is_none() {
                self.failed = self.candidate.take().map(|(_, hit)| hit);
            }
        } else if (2..=4).contains(&rank)
            && self.candidate.as_ref().is_none_or(|(old, _)| rank > *old)
        {
            let (preview, preview_shortened) = shorten(line, 512);
            self.candidate = Some((
                rank,
                LogMatch {
                    line: number,
                    preview,
                    preview_shortened,
                    command: format!(
                        "{} --start {number} --lines {count}",
                        reference.command.clone()
                    ),
                    test_name: None,
                    test_output_command: None,
                },
            ));
        }
    }
}

// Only recognize markers at the start of actual log payload, not words in embedded prose.
fn failure_rank(text: &str) -> u8 {
    if go_event(text).is_some_and(|(event, _)| event == "FAIL") {
        return 5;
    }
    if text.starts_with("##[error]Process completed with exit code") {
        return 1;
    }
    if text.starts_with("##[error]") {
        return 4;
    }
    if text.ends_with(" (ERROR)") {
        return 3;
    }
    // Standard date/time-prefixed logger output, rather than an ERROR word in prose.
    let mut fields = text.split_whitespace();
    if let (Some(date), Some(clock), Some("ERROR")) = (fields.next(), fields.next(), fields.next())
        && date.len() == 10
        && date
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'/' || byte == b'-')
        && clock.len() == 8
        && clock
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b':')
    {
        return 3;
    }
    if ["panic:", "AssertionError", "FAILED ", "test result: FAILED"]
        .iter()
        .any(|marker| text.starts_with(marker))
    {
        return 2;
    }
    0
}

fn payload(line: &str) -> &str {
    let line = line.trim_start();
    if let Some((timestamp, rest)) = line.split_once(' ')
        && timestamp.ends_with('Z')
        && timestamp.contains('T')
        && timestamp.as_bytes().first().is_some_and(u8::is_ascii_digit)
    {
        return rest.trim_start();
    }
    line
}

fn go_event(text: &str) -> Option<(&'static str, &str)> {
    for (prefix, event) in [
        ("=== RUN", "RUN"),
        ("=== NAME", "NAME"),
        ("=== CONT", "CONT"),
        ("=== PAUSE", "PAUSE"),
        ("--- FAIL:", "FAIL"),
        ("--- PASS:", "PASS"),
        ("--- SKIP:", "SKIP"),
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            let name = rest.split_whitespace().next()?;
            if name.len() <= 1024 {
                return Some((event, name));
            }
        }
    }
    None
}

fn attributed(payload: &str, active: &mut Option<String>, test: &str) -> bool {
    if let Some((event, name)) = go_event(payload) {
        match event {
            "RUN" | "NAME" | "CONT" => *active = Some(name.into()),
            _ if active.as_deref() == Some(name) => *active = None,
            _ => {}
        }
        return name == test;
    }
    active.as_deref() == Some(test)
}

fn assertion_anchor(file: &mut File, hits: &[LogMatch]) -> Result<Option<usize>> {
    let names: Vec<_> = hits
        .iter()
        .filter_map(|hit| hit.test_name.clone())
        .collect();
    let names: Vec<_> = names
        .iter()
        .filter(|name| {
            !names
                .iter()
                .any(|other| other.starts_with(&format!("{name}/")))
        })
        .cloned()
        .collect();
    assertion_for(file, &names)
}

fn assertion_for(file: &mut File, names: &[String]) -> Result<Option<usize>> {
    if names.is_empty() {
        return Ok(None);
    }
    let mut active = None;
    let mut fallback = None;
    for (index, line) in log_lines(file).enumerate() {
        let line = line?;
        let text = payload(&line);
        if let Some((event, name)) = go_event(text) {
            active = matches!(event, "RUN" | "NAME" | "CONT").then(|| name.to_owned());
        } else if active.as_ref().is_some_and(|name| names.contains(name)) && diagnostic(text) {
            let lower = text.to_ascii_lowercase();
            if [
                "failed",
                "error",
                "cannot",
                "could not",
                "got ",
                "want ",
                "expected",
                "unknown",
                "exit",
                "unreachable",
                "unable",
            ]
            .iter()
            .any(|marker| lower.contains(marker))
            {
                return Ok(Some(index + 1));
            }
            fallback.get_or_insert(index + 1);
        }
    }
    Ok(fallback)
}

fn diagnostic(text: &str) -> bool {
    let Some((file, rest)) = text.split_once(':') else {
        return false;
    };
    let Some((number, _)) = rest.split_once(':') else {
        return false;
    };
    std::path::Path::new(file)
        .extension()
        .is_some_and(|extension| extension == "go")
        && !file.contains(char::is_whitespace)
        && !number.is_empty()
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && !text.contains("level=")
        && !text.contains("\"level\"")
}

type Excerpt = (Vec<String>, Vec<usize>, usize, bool, Option<usize>);
fn excerpt(file: &mut File, offset: usize, count: usize, test: Option<&str>) -> Result<Excerpt> {
    let mut lines = Vec::new();
    let mut numbers = Vec::new();
    let mut shortened = 0;
    let mut bytes = 0;
    let mut active = None;
    for (index, line) in log_lines(file).enumerate() {
        let line = line?;
        let selected = test.is_none_or(|test| attributed(payload(&line), &mut active, test));
        if index < offset || !selected {
            continue;
        }
        let (preview, cut) = shorten(&line, 2048);
        let byte_limit = bytes + preview.len() + 1 > 16 * 1024;
        if lines.len() == count || byte_limit {
            return Ok((lines, numbers, shortened, byte_limit, Some(index + 1)));
        }
        bytes += preview.len() + 1;
        shortened += usize::from(cut);
        lines.push(preview);
        numbers.push(index + 1);
    }
    Ok((lines, numbers, shortened, false, None))
}

fn shorten(line: &str, limit: usize) -> (String, bool) {
    let mut preview = String::new();
    for grapheme in line.graphemes(true) {
        if preview.len() + grapheme.len() > limit {
            break;
        }
        preview.push_str(grapheme);
    }
    let shortened = preview.len() < line.len();
    if shortened {
        preview.push_str(" [shortened; see Full log]");
    }
    (preview, shortened)
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
fn log_lines(file: &mut File) -> impl Iterator<Item = std::io::Result<String>> + '_ {
    std::io::BufReader::with_capacity(64 * 1024, file)
        .split(b'\n')
        .map(|bytes| {
            bytes.map(|mut bytes| {
                if bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
                String::from_utf8_lossy(&bytes).into_owned()
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn failed_step_diagnostics_outrank_setup_and_cleanup_errors() {
        let reference: CheckRef =
            "job:github.com/example/project:-:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:20:1:30"
                .parse()
                .unwrap();
        let mut nav = Navigation::job(&reference);
        nav.failed_steps
            .push(("2026-01-01T00:01:00Z".into(), "2026-01-01T00:01:59Z".into()));
        for diagnostic in [
            "##[error] invalid input cafe\u{301}",
            "invalid input 👩‍💻 (ERROR)",
        ] {
            let text = format!(
                "2026-01-01T00:00:00Z ##[error] setup noise\n{}2026-01-01T00:01:30.123Z {diagnostic}\n{}2026-01-01T00:02:00Z ##[error] cleanup noise\n",
                "2026-01-01T00:01:01Z progress\n".repeat(30),
                "2026-01-01T00:01:40Z progress\n".repeat(30)
            );
            let mut file = crate::cache::spool().unwrap();
            file.write_all(text.as_bytes()).unwrap();
            file.rewind().unwrap();
            let result = page(
                &mut file,
                &nav,
                None,
                10,
                LogSource::Cache,
                1,
                Query::Window,
            )
            .unwrap();
            assert!(result.lines.iter().any(|line| line.contains(diagnostic)));
            assert!(!result.lines.iter().any(|line| line.contains("noise")));
            assert!(result.navigation_note.contains("failed step"));
            file.rewind().unwrap();
            let explicit = page(
                &mut file,
                &nav,
                Some(1),
                1,
                LogSource::Cache,
                1,
                Query::Window,
            )
            .unwrap();
            assert!(explicit.lines.first().unwrap().contains("setup noise"));
            file.rewind().unwrap();
            let search = page(
                &mut file,
                &nav,
                None,
                10,
                LogSource::Cache,
                1,
                Query::Search("noise"),
            )
            .unwrap();
            assert_eq!(search.matches, 2, "search still covers setup and cleanup");
        }
        let tail = view("ordinary output\n", Query::Window, None, 10);
        assert!(tail.navigation_note.contains("Tail fallback"));
    }

    fn view(text: &str, query: Query<'_>, start: Option<usize>, count: usize) -> LogPage {
        let mut file = crate::cache::spool().unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file.rewind().unwrap();
        let reference: CheckRef =
            "job:github.com/example/project:7:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:20:-:30"
                .parse()
                .unwrap();
        page(
            &mut file,
            &Navigation::job(&reference),
            start,
            count,
            LogSource::Cache,
            1,
            query,
        )
        .unwrap()
    }

    #[test]
    fn search_context_keeps_preceding_lines_and_match_within_both_budgets() {
        let text = format!(
            "{}\npackage header\nsrc/café.go:7: undefined: symbol\ncleanup",
            format!("{}\n", "👨‍👩‍👧".repeat(1000)).repeat(12)
        );
        for count in [1, 2, 3, 20] {
            let index = view(&text, Query::Search("undefined:"), None, count);
            let hit = index.match_index.first().unwrap();
            let start = hit
                .command
                .split("--start ")
                .nth(1)
                .unwrap()
                .split_whitespace()
                .next()
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(
                start,
                hit.line.saturating_sub(count.saturating_sub(1).min(3))
            );
            let context = view(&text, Query::Window, Some(start), count);
            assert!(context.lines.iter().any(|line| line.contains("undefined:")));
            if count > 1 {
                assert!(context.lines.iter().any(|line| line == "package header"));
            }
        }
    }

    #[test]
    fn structured_failures_outrank_expected_errors_and_embedded_prose() {
        let text = format!(
            "=== RUN   TestExpected\n##[error] expected negative probe\n--- PASS: TestExpected (0.1s)\n{}\n=== RUN   TestParent\nsetup.go:1: setup\n=== RUN   TestParent/TestBroken\nsource.go:1: starting test\nsource.go:12: got 4, want 5\n{}\n--- FAIL: TestParent (1s)\n--- FAIL: TestParent/TestBroken (1s)\n",
            "agent said ##[error] untrusted prose\n".repeat(50),
            "cleanup\n".repeat(50)
        );
        let page = view(&text, Query::Window, None, 6);
        assert!(page.lines.iter().any(|line| line.contains("got 4, want 5")));
        assert!(
            !page
                .lines
                .iter()
                .any(|line| line.contains("negative probe"))
        );
        assert_eq!(
            page.match_index.first().unwrap().test_name.as_deref(),
            Some("TestParent")
        );
    }

    #[test]
    fn annotation_matching_ignores_embedded_agent_prose() {
        let text = format!(
            "agent reasoning: ##[error] perhaps a failure\n{}\n2026-01-01T00:00:00Z ##[error] score 0.8 below minimum 0.9\n",
            "reasoning\n".repeat(80)
        );
        let page = view(&text, Query::Window, None, 5);
        assert!(page.lines.iter().any(|line| line.contains("score 0.8")));
    }

    #[test]
    fn timestamped_gate_error_outranks_generic_exit_wrapper() {
        let text = format!(
            "agent prose mentions ERROR\n2026-01-01T00:00:00Z 2026/01/01 00:00:00 ERROR contracted score failed\n  - score: 0.8 below minimum 0.9\n{}\n##[error]Process completed with exit code 1.",
            "report body\n".repeat(80)
        );
        let page = view(&text, Query::Window, None, 5);
        assert!(page.lines.iter().any(|line| line.contains("0.8 below")));
    }

    #[test]
    fn compact_search_counts_cursor_and_marks_truncated_previews() {
        let text = format!("needle\n{} needle\nneedle\n", "cafe\u{301} 👨‍👩‍👧".repeat(100));
        let page = view(&text, Query::Search("needle"), Some(2), 100);
        assert_eq!(page.matches, 3);
        assert_eq!(page.eligible_matches, 2);
        assert!(page.lines.is_empty());
        assert!(page.next_command.is_none());
        assert!(page.previous_command.is_none());
        let hit = page.match_index.first().unwrap();
        assert!(hit.preview_shortened);
        assert!(hit.preview.ends_with("[shortened; see Full log]"));
        assert!(hit.command.contains("--start 1"));
    }

    #[test]
    fn test_output_rejoins_regions_and_pages_original_line_numbers() {
        let text = "=== RUN   TestA\na.go:1: assertion\n=== PAUSE TestA\n=== RUN   TestB\nb.go:2: unrelated\n=== NAME  TestA\na.go:3: cafe\u{301}\n=== NAME  TestB\nb.go:4: unrelated\n=== CONT  TestA\na.go:5: more\n--- FAIL: TestA (1s)\n";
        let page = view(text, Query::Test("TestA"), None, 3);
        assert_eq!(page.line_numbers, [2, 3, 6]);
        assert!(!page.lines.iter().any(|line| line.contains("unrelated")));
        assert!(
            page.next_command
                .unwrap()
                .contains("--test 'TestA' --start 7")
        );
        let page = view(text, Query::Test("TestA"), Some(7), 10);
        assert_eq!(page.line_numbers, [7, 10, 11, 12]);
        assert!(page.next_command.is_none());
    }

    #[test]
    fn supports_tests_skips_setup_diagnostics_and_keeps_failure_visible() {
        let text = format!(
            "=== RUN   TestExample\nsetup.go:1: initialized\n{}\nassertion.go:99: expected two, got one\n--- FAIL: TestExample (1s)",
            "noisy setup\n".repeat(50)
        );
        let page = view(&text, Query::Test("TestExample"), None, 2);
        assert!(
            page.lines
                .first()
                .unwrap()
                .contains("expected two, got one")
        );
        let text = format!(
            "=== RUN   TestExample\n{}\nassertion.go:99: expected two, got one\n--- FAIL: TestExample (1s)",
            format!("{}\n", "x".repeat(2000)).repeat(50)
        );
        let page = view(&text, Query::Window, None, 100);
        assert!(
            page.lines
                .iter()
                .any(|line| line.contains("expected two, got one"))
        );
    }

    #[test]
    fn absent_test_returns_no_unrelated_tail() {
        let page = view(
            "=== RUN   TestB\nb.go:2: output\n--- PASS: TestB",
            Query::Test("TestA"),
            None,
            40,
        );
        assert!(page.lines.is_empty());
        assert!(page.line_numbers.is_empty());
        assert!(page.next_command.is_none());
    }

    #[test]
    fn aggregate_marker_is_detected_outside_excerpt() {
        let text = format!(
            "needs results: success success failure\n{}\n##[error] aggregate failed\n",
            "cleanup\n".repeat(100)
        );
        let page = view(&text, Query::Window, None, 5);
        assert!(page.dependency_marker);
        assert!(
            !page
                .lines
                .iter()
                .any(|line| line.starts_with("needs results"))
        );
    }
}
