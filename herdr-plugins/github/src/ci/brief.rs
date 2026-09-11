use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context as _, Result, bail};

use crate::github::{self, CheckBucket, CiState, Lookup};

const LOG_LINES: usize = 150;
const MAX_LOGGED: usize = 3;

pub fn build(dir: &Path) -> Result<String> {
    let Lookup::Found(state) = github::lookup(dir)? else {
        bail!("there is no pull request for this branch");
    };
    build_for_state(dir, &state)
}

pub(crate) fn build_for_state(dir: &Path, state: &CiState) -> Result<String> {
    let head = short(&state.local_head_oid);

    let mut out = String::new();
    writeln!(out, "# CI failures on PR #{}\n", state.number)?;
    writeln!(out, "- branch: `{}`", state.local_branch)?;
    writeln!(out, "- head: `{head}`")?;
    writeln!(out, "- review: {}\n", state.review.label())?;

    let mut failing: Vec<_> = state
        .checks
        .iter()
        .filter(|check| matches!(check.bucket, CheckBucket::Fail | CheckBucket::Cancel))
        .collect();
    failing.sort_by_key(|check| !state.is_required(&check.name));
    if failing.is_empty() {
        out.push_str("No checks are failed or cancelled. Nothing to fix.\n");
        return Ok(out);
    }

    let mut logged = 0;
    for check in failing {
        let required_mark = if state.is_required(&check.name) {
            " **(required)**"
        } else {
            ""
        };
        writeln!(out, "## {}{required_mark}\n", check.name)?;
        writeln!(out, "- result: {}", bucket_name(check.bucket))?;
        if !check.workflow.is_empty() {
            writeln!(out, "- workflow: {}", check.workflow)?;
        }
        if !check.link.is_empty() {
            writeln!(out, "- link: {}", check.link)?;
        }
        out.push('\n');

        let Some((owner_repo, run)) = action_run(&check.link) else {
            out.push_str("Not a GitHub Actions check, so no logs are retrievable here. Open the link above to read the failure; do not guess at the cause.\n\n");
            continue;
        };
        if logged >= MAX_LOGGED {
            writeln!(
                out,
                "Logs omitted (budget of {MAX_LOGGED} checks reached). Fetch with:\n\n    gh run view {run} -R {owner_repo} --log-failed\n"
            )?;
            continue;
        }
        out.push_str("```\n");
        match github::run_log(dir, &owner_repo, &run) {
            Ok(log) => out.push_str(&log_excerpt(&log)),
            Err(error) => {
                eprintln!("github: could not fetch logs for {}: {error:#}", check.name);
                out.push_str("(could not fetch logs)\n");
            }
        }
        out.push_str("```\n\n");
        logged += 1;
    }
    Ok(out)
}

pub fn show(dir: &Path) -> Result<()> {
    let brief = build(dir)?;
    let mut child = Command::new("less")
        .arg("-R")
        .stdin(Stdio::piped())
        .spawn()
        .context("starting less")?;
    let mut stdin = child.stdin.take().context("opening less stdin")?;
    let written = stdin.write_all(brief.as_bytes());
    drop(stdin);
    let status = child.wait().context("waiting for less")?;
    if let Err(error) = written
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(error).context("writing the CI brief to less");
    }
    if !status.success() {
        bail!("less exited with {status}");
    }
    Ok(())
}

fn bucket_name(bucket: CheckBucket) -> &'static str {
    match bucket {
        CheckBucket::Pass => "pass",
        CheckBucket::Fail => "fail",
        CheckBucket::Pending => "pending",
        CheckBucket::Skipping => "skipping",
        CheckBucket::Cancel => "cancel",
        CheckBucket::Unknown => "unknown",
    }
}

fn short(head: &str) -> &str {
    head.get(..9).unwrap_or(head)
}

fn action_run(link: &str) -> Option<(String, String)> {
    let path = link.strip_prefix("https://github.com/")?;
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next()? != "actions" || parts.next()? != "runs" {
        return None;
    }
    let run = parts.next()?.split('?').next()?;
    if !github_path_segment(owner)
        || !github_path_segment(repo)
        || !run.bytes().all(|byte| byte.is_ascii_digit())
        || run.is_empty()
    {
        return None;
    }
    Some((format!("{owner}/{repo}"), run.to_owned()))
}

fn github_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn log_excerpt(log: &str) -> String {
    let lines: Vec<&str> = log
        .lines()
        .map(|line| line.splitn(4, '\t').nth(3).unwrap_or(line))
        .collect();
    let (start, count) = lines
        .iter()
        .position(|line| line.contains("##[error]"))
        .map_or_else(
            || (lines.len().saturating_sub(LOG_LINES), LOG_LINES),
            |at| (at.saturating_sub(40), 61),
        );
    let mut out = lines
        .iter()
        .skip(start)
        .take(count)
        .copied()
        .collect::<Vec<_>>()
        .join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_actions_url_yields_the_repo_and_run() {
        assert_eq!(
            action_run(
                "https://github.com/example/service/actions/runs/123?check_suite_focus=true"
            ),
            Some(("example/service".to_owned(), "123".to_owned()))
        );
        assert_eq!(action_run("https://checks.example.test/run/123"), None);
        assert_eq!(
            action_run("https://github.com/example/service/actions/runs/--repo"),
            None
        );
        assert_eq!(
            action_run("https://github.com/example%2Fevil/service/actions/runs/123"),
            None
        );
    }

    #[test]
    fn a_log_excerpt_starts_before_the_first_error() {
        let log = (0..80)
            .map(|n| {
                let message = if n == 60 { "##[error] broken" } else { "ok" };
                format!("check\tstep\tstamp\t{n} {message}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let excerpt = log_excerpt(&log);
        assert!(excerpt.starts_with("20 ok\n"));
        assert!(excerpt.contains("60 ##[error] broken\n"));
    }
}
