use crate::command::{ReviewArgs, ReviewCommand};
use anyhow::Result;
use github_inspect::{GitHub, ObservationSource, Repository, reviews};
use std::fmt::Write as _;

pub(super) fn run(
    provider: &GitHub<'_>,
    args: ReviewArgs,
    repo: Option<&Repository>,
    json: bool,
) -> Result<String> {
    match args.command {
        None => {
            let value = reviews::list(
                provider,
                reviews::ListOptions {
                    pr: args.list.pr.as_deref(),
                    repository: repo,
                    snapshot: args.list.snapshot.as_ref(),
                    all: args.list.all,
                    offset: args.list.offset as usize,
                    limit: usize::from(args.list.limit),
                },
            )?;
            if json {
                return Ok(crate::json::render(serde_json::to_value(value)?));
            }
            Ok(render_listing(value))
        }
        Some(ReviewCommand::Thread(args)) => {
            let value = reviews::thread(
                provider,
                &args.reference,
                reviews::ThreadOptions {
                    snapshot: args.snapshot.as_ref(),
                    offset: args.offset as usize,
                    limit: usize::from(args.limit),
                },
            )?;
            if json {
                return Ok(crate::json::render(serde_json::to_value(value)?));
            }
            Ok(render_thread(value))
        }
    }
}
fn observation(out: &mut String, source: &ObservationSource, time: u64) {
    let _ = writeln!(
        out,
        "Observed at {time}{}.",
        if matches!(source, ObservationSource::Snapshot) {
            "; saved observation, GitHub was not queried"
        } else {
            ""
        }
    );
}
fn footer(out: &mut String, next: Option<&str>, refresh: &str, warnings: &[String]) {
    for warning in warnings {
        let _ = writeln!(out, "Warning: {warning}");
    }
    if let Some(next) = next {
        let _ = writeln!(out, "Continue: {next}");
    }
    let _ = writeln!(out, "Refresh: {refresh}");
}

fn render_listing(value: reviews::Listing) -> String {
    let mut out = format!(
        "{}\n{} threads; {} unresolved ({} outdated). Showing {} of {} matching.\n",
        value.subject.url,
        value.summary.total,
        value.summary.unresolved,
        value.summary.unresolved_outdated,
        value.returned,
        value.matching
    );
    observation(&mut out, &value.source, value.observed_at);
    if value.threads.is_empty() {
        out.push_str("No matching threads on this page.\n");
    }
    for thread in value.threads {
        let _ = writeln!(
            out,
            "\n{}:{:?} · resolved={} outdated={} · {} comments",
            thread.location.path,
            thread.location.line,
            thread.resolved,
            thread.outdated,
            thread.comments_total
        );
        if let Some(comment) = thread.preview {
            let _ = writeln!(
                out,
                "{}: {}{}",
                comment.author.as_deref().unwrap_or("[deleted]"),
                comment.body.text,
                if comment.body.truncated {
                    " [preview truncated]"
                } else {
                    ""
                }
            );
        }
        let _ = writeln!(out, "Read: {}", thread.next_command);
    }
    footer(
        &mut out,
        value.next_command.as_deref(),
        &value.refresh_command,
        &value.warnings,
    );
    crate::render::printable(&out)
}
fn render_thread(value: reviews::Thread) -> String {
    let mut out = format!(
        "{}:{:?} · resolved={} outdated={}\nPR head: {}\nShowing {} of {} comments.\n",
        value.location.path,
        value.location.line,
        value.resolved,
        value.outdated,
        value.subject.head,
        value.returned,
        value.comments_total
    );
    observation(&mut out, &value.source, value.observed_at);
    if let Some(context) = value.context {
        let _ = writeln!(
            out,
            "\nReviewed diff (original commit {}):\n{}{}",
            context
                .original_commit_id
                .as_deref()
                .unwrap_or("unavailable"),
            context.diff_hunk.text,
            if context.diff_hunk.truncated {
                "\n[diff truncated; see comment URL]"
            } else {
                ""
            }
        );
        if context.diff_hunk.truncated {
            let _ = writeln!(
                out,
                "Full review diff hunk: {}",
                context
                    .full_diff_hunk_command
                    .as_deref()
                    .unwrap_or(&context.url)
            );
        }
    }
    if let Some(command) = value.context_command {
        let _ = writeln!(out, "Original review context: {command}");
    }
    for comment in value.comments {
        let _ = writeln!(
            out,
            "\n{} · {}\n{}\n{}",
            comment.author.as_deref().unwrap_or("[deleted]"),
            comment.created_at,
            comment.body.text,
            comment.url
        );
        if comment.body.truncated {
            let _ = writeln!(
                out,
                "Body truncated. Full comment: {}",
                comment
                    .full_comment_command
                    .as_deref()
                    .unwrap_or(&comment.url)
            );
        }
    }
    footer(
        &mut out,
        value.next_command.as_deref(),
        &value.refresh_command,
        &value.warnings,
    );
    crate::render::printable(&out)
}
