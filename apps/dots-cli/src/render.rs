use github_inspect::{Evidence, Inspection};
use std::fmt::Write as _;

pub(super) fn inspection(value: &Inspection) -> String {
    let Some(subject) = &value.subject else {
        return "No PR found for this branch. Select --pr, --ref, --sha or --run explicitly.\n"
            .into();
    };
    let mut out = subject_header(subject);
    let _ = writeln!(out, "Observed: {}", observed_time(value.observed_at));
    if matches!(value.source, github_inspect::ObservationSource::Snapshot) {
        out.push_str("Saved observation: current GitHub status was not queried.\n");
    }
    let summary = &value.summary;
    let _ = writeln!(
        out,
        "CI: {} failed, {} pending, {} passed, {} cancelled, {} skipped, {} unknown",
        summary.failed,
        summary.pending,
        summary.passed,
        summary.cancelled,
        summary.skipped,
        summary.unknown
    );
    render_local_head(value, subject.head(), &mut out);
    for warning in &value.warnings {
        let _ = writeln!(out, "Warning: {warning}");
    }
    if value.matching_checks == 0 && value.total_checks > 0 {
        out.push_str("No checks match the detail filter.\n");
    } else if value.checks.is_empty() && value.total_checks > 0 {
        let _ = writeln!(
            out,
            "No checks on this page (offset {}; {} total).",
            value.offset, value.matching_checks
        );
    } else if value.omitted_checks > 0 {
        let _ = writeln!(
            out,
            "Showing checks {}–{} of {} (failure-first); {} omitted. Follow More checks to continue.",
            value.offset + 1,
            value.offset + value.checks.len(),
            value.matching_checks,
            value.omitted_checks
        );
    }
    if value.hidden_checks > 0 {
        let _ = writeln!(
            out,
            "{} checks hidden by the detail filter; summary covers all {}.",
            value.hidden_checks, value.total_checks
        );
        if let Some(command) = &value.all_checks_command {
            let _ = writeln!(out, "All states: {command}");
        }
    }
    if value.total_checks == 0 {
        out.push_str("No checks reported. This is not evidence that CI passed.\n");
    }
    for check in &value.checks {
        let required = match check.required {
            Some(true) => "required",
            Some(false) => "optional",
            None if subject.pr().is_none() => "requiredness not evaluated outside PR context",
            None => "requiredness unknown",
        };
        let _ = writeln!(
            out,
            "\n{:?} | {} | {} | {required}",
            check.bucket, check.workflow, check.name
        );
        if let Some(status) = &check.status {
            let _ = writeln!(out, "  Status: {status}");
        }
        if check.next_command.is_none() {
            let _ = writeln!(out, "  {}", check.link);
        }
        if let Some(command) = &check.next_command {
            let _ = writeln!(out, "  More: {command}");
        }
        if let Some(command) = &check.output_command
            && check.reference.is_some()
        {
            let _ = writeln!(out, "  Check output: {command}");
        }
        if let Some(note) = &check.evidence_note {
            let _ = writeln!(out, "  {note}");
        }
        if let Some(value) = &check.evidence {
            out.push_str(&evidence(value));
        }
        if let Some(value) = &check.output_evidence {
            out.push_str(&check_output(value));
        }
    }
    if let Some(command) = &value.next_command {
        let _ = writeln!(out, "\nMore checks: {command}");
    }
    printable(&out)
}

fn subject_header(subject: &github_inspect::Subject) -> String {
    match subject {
        github_inspect::Subject::PullRequest { pr, .. } => format!(
            "{} ({})\nPR head: {}\nScope: PR checks.\n",
            pr.url, pr.state, pr.head_ref_oid
        ),
        github_inspect::Subject::Commit {
            sha, requested_ref, ..
        } => format!(
            "{}\nRevision: {sha}\nResolved ref: {}\nScope: reported checks/statuses on this commit; not branch history or merge eligibility.\n",
            subject.url(),
            requested_ref.as_deref().unwrap_or("exact SHA")
        ),
        github_inspect::Subject::Run {
            attempt,
            sha,
            name,
            event,
            branch,
            status,
            conclusion,
            ..
        } => format!(
            "{}\nWorkflow: {name} (attempt {attempt})\nEvent: {event}; branch: {}\nRevision: {sha}\nRun status: {status} / {}\nScope: this workflow attempt; counts cover reported jobs/check output.\n",
            subject.url(),
            branch.as_deref().unwrap_or("none"),
            conclusion.as_deref().unwrap_or("unknown")
        ),
    }
}

fn render_local_head(value: &Inspection, head: &str, out: &mut String) {
    if let Some(local) = &value.local_head {
        let _ = writeln!(
            out,
            "Local head: {local}{}",
            if local == head {
                " (matches inspected revision)"
            } else {
                " (differs from inspected revision; inspect before editing)"
            }
        );
    }
}

pub(super) fn evidence(value: &Evidence) -> String {
    let mut out = format!(
        "\nJob: {} (attempt {})\nJob head: {}\nObserved revision: {}\nStatus: {} / {}\n",
        value.job_name,
        value.run_attempt,
        value.job_head,
        value.observed_head,
        value.status,
        value.conclusion.as_deref().unwrap_or("unknown")
    );
    for warning in &value.warnings {
        let _ = writeln!(out, "Warning: {warning}");
    }
    for step in &value.steps {
        if step
            .conclusion
            .as_deref()
            .is_some_and(|s| s != "success" && s != "skipped")
        {
            let _ = writeln!(
                out,
                "Step {}: {} ({})",
                step.number,
                step.name,
                step.conclusion.as_deref().unwrap_or("unknown")
            );
        }
    }
    let _ = writeln!(out, "Observed: {}", observed_time(value.observed_at));
    if matches!(value.source, github_inspect::ObservationSource::Snapshot) {
        out.push_str("Saved job evidence: GitHub was not queried.\n");
    }
    render_log(value, &mut out);
    let _ = writeln!(out, "Search: {} --search 'TEXT'", value.command());
    if value.snapshot.is_some() {
        let _ = writeln!(out, "Refresh: {}", value.reference.command());
    }
    let _ = writeln!(out, "Full log: {}", value.full_log_command);
    printable(&out)
}

fn render_log(value: &Evidence, out: &mut String) {
    if let Some(log) = &value.log {
        render_page(log, &value.command(), "Log", out);
    } else {
        out.push_str("No log evidence retrieved; do not infer a cause from status alone.\n");
    }
}

fn render_page(log: &github_inspect::LogPage, command: &str, label: &str, out: &mut String) {
    let source = match log.source {
        github_inspect::LogSource::Network => "network",
        github_inspect::LogSource::Cache => "cache",
    };
    let _ = writeln!(
        out,
        "{label} source: {source}; fetched at {}.",
        observed_time(log.fetched_at)
    );
    if let Some(search) = &log.search {
        let _ = writeln!(
            out,
            "Search {search:?}: {} matches at/after cursor; {} in the complete log.",
            log.eligible_matches, log.matches
        );
        for hit in &log.match_index {
            let _ = writeln!(
                out,
                "Match {}: {}\n  Context: {}",
                hit.line, hit.preview, hit.command
            );
            if let Some(command) = &hit.test_output_command {
                let _ = writeln!(out, "  Test output: {command}");
            }
        }
        if log.eligible_matches == 0 {
            out.push_str("No matching lines at or after the requested start.\n");
        }
    }
    if log.byte_limit_reached {
        out.push_str("Excerpt byte limit reached; Continue starts at the first omitted line.\n");
    }
    if log.shortened_lines > 0 {
        let _ = writeln!(
            out,
            "{} long lines shortened. Full log: {}",
            log.shortened_lines, log.full_log_command
        );
    }
    let _ = writeln!(out, "{}", log.navigation_note);
    if log.search.is_none() && !log.lines.is_empty() {
        for hit in &log.match_index {
            if let Some(command) = &hit.test_output_command {
                let _ = writeln!(
                    out,
                    "Failed test: {}\n  Test output: {command}",
                    hit.test_name.as_deref().unwrap_or("unknown")
                );
            }
        }

        let _ = writeln!(
            out,
            "Evidence ({} lines omitted before, {} after):",
            log.omitted_before, log.omitted_after
        );
        for (index, line) in log.lines.iter().enumerate() {
            let _ = writeln!(
                out,
                "{}: {line}",
                log.line_numbers
                    .get(index)
                    .copied()
                    .unwrap_or(log.start_line + index)
            );
        }
    }
    if log.test.is_some() && log.lines.is_empty() {
        out.push_str("No attributed test output at or after the requested start.\n");
    }
    if let Some(previous) = &log.previous_command {
        let _ = writeln!(out, "Previous: {previous}");
    }
    if let Some(next) = &log.next_match_command {
        let _ = writeln!(out, "More matches: {next}");
    }
    if let Some(next) = &log.next_command {
        let _ = writeln!(out, "Continue: {next}");
    }
    if log.omitted_before > 0 && log.test.is_none() && log.search.is_none() {
        let _ = writeln!(out, "From beginning: {command} --start 1");
    }
}

pub(super) fn check_output(value: &github_inspect::CheckOutput) -> String {
    let mut out = format!(
        "\nCheck: {}\nCheck head: {}\nObserved revision: {}\nStatus: {} / {}\nObserved: {}\n",
        value.name,
        value.check_head,
        value.observed_head,
        value.status,
        value.conclusion.as_deref().unwrap_or("unknown"),
        observed_time(value.observed_at)
    );
    if matches!(value.source, github_inspect::ObservationSource::Snapshot) {
        out.push_str("Saved check output: GitHub was not queried.\n");
    }
    for warning in &value.warnings {
        let _ = writeln!(out, "Warning: {warning}");
    }
    if let Some(log) = &value.output {
        let target = value.command();
        render_page(log, &target, "Output", &mut out);
        let _ = writeln!(out, "Search: {target} --search 'TEXT'");
    }
    if let Some(annotations) = &value.annotations {
        if annotations.is_empty() {
            out.push_str("No annotations on this page.\n");
        }
        for item in annotations {
            let _ = writeln!(
                out,
                "{}:{}–{} [{}] {}\n{}{}",
                item.path,
                item.start_line,
                item.end_line,
                item.level,
                item.title.as_deref().unwrap_or_default(),
                item.message,
                if item.shortened {
                    " [shortened; see Full output]"
                } else {
                    ""
                }
            );
        }
    }
    if let Some(command) = &value.annotations_command {
        let _ = writeln!(out, "Annotations ({}): {command}", value.annotations_count);
    }
    if let Some(command) = &value.next_annotations_command {
        let _ = writeln!(out, "More annotations: {command}");
    }
    if let Some(url) = &value.provider_url {
        let _ = writeln!(out, "Provider: {url}");
    }
    if value.snapshot.is_some() {
        let _ = writeln!(out, "Refresh: {}", value.reference.command());
    }
    let _ = writeln!(out, "Full output: {}", value.full_output_command);
    printable(&out)
}

// Provider-controlled names and logs may contain terminal controls. Preserve
// them visibly instead of allowing output to rewrite surrounding evidence.
pub(super) fn printable(text: &str) -> String {
    strip_sgr(text)
        .chars()
        .flat_map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\t') {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

// Remove only ANSI SGR styling. Other controls remain visible and inert.
fn strip_sgr(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\u{1b}[") {
        let (before, sequence) = rest.split_at(at);
        out.push_str(before);
        let body = sequence.strip_prefix("\u{1b}[").unwrap_or(sequence);
        let params = body
            .bytes()
            .take_while(|byte| byte.is_ascii_digit() || matches!(byte, b';' | b':'))
            .count();
        if body.get(params..).is_some_and(|tail| tail.starts_with('m')) {
            rest = body.get(params + 1..).unwrap_or_default();
        } else {
            out.push_str("\u{1b}[");
            rest = body;
        }
    }
    out.push_str(rest);
    out
}

pub(crate) fn observed_time(seconds: u64) -> String {
    i64::try_from(seconds)
        .ok()
        .and_then(|seconds| time::OffsetDateTime::from_unix_timestamp(seconds).ok())
        .map_or_else(
            || format!("{seconds} (Unix seconds)"),
            |date| {
                format!(
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                    date.year(),
                    u8::from(date.month()),
                    date.day(),
                    date.hour(),
                    date.minute(),
                    date.second()
                )
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_is_removed_but_terminal_commands_remain_visible() {
        assert_eq!(printable("\u{1b}[36;1mcafe\u{301}\u{1b}[0m"), "cafe\u{301}");
        assert_eq!(printable("\u{1b}[2J"), "\\u{1b}[2J");
        assert!(!printable("\u{1b}]8;;https://example.test\u{7}link").contains('\u{1b}'));
    }

    #[test]
    fn controls_cannot_overwrite_evidence_and_unicode_survives() {
        let text = printable("\u{1b}[2Jcafe\u{301} 👨‍👩‍👧\rhidden\n");
        assert!(!text.contains('\u{1b}') && !text.contains('\r'));
        assert!(text.contains("cafe\u{301} 👨‍👩‍👧"));
        assert!(text.contains("\\rhidden\n"));
    }
}
