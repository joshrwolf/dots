//! `dots review`: parse requests, call the review store, and present results.
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use dots_review::{
    ChangedFile, NewThread, Overview, Refreshed, Reviews, Side, Status, Summary, TargetSpec, Thread,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;

use crate::command::{ReviewBody, ReviewCommand, ReviewTarget};

/// Largest threads file `add`, or body file `reply`, reads.
const INPUT_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    threads: Vec<NewThread>,
}

pub(crate) fn run(command: ReviewCommand, directory: &Path, json: bool) -> Result<String> {
    let reviews = Reviews::open(directory)?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let id = |id: Option<String>| id.map_or_else(|| reviews.latest(), Ok);
    match command {
        ReviewCommand::Start {
            target,
            profile,
            owner,
        } => {
            let owner = owner.or_else(crate::agent::session);
            let overview = reviews.start(spec(target)?, profile, owner, now)?;
            Ok(if json {
                crate::json::render(serde_json::to_value(&overview)?)
            } else {
                render_overview(&overview)
            })
        }
        ReviewCommand::List { all, owner } => {
            let mut summaries = reviews.list(all)?;
            summaries.retain(|summary| owner.is_none() || summary.owner == owner);
            Ok(if json {
                crate::json::render(json!({ "reviews": summaries }))
            } else {
                render_list(&summaries, all, owner.as_deref())
            })
        }
        ReviewCommand::Show { id: review } => {
            let overview = reviews.show(&id(review)?)?;
            Ok(if json {
                crate::json::render(serde_json::to_value(&overview)?)
            } else {
                render_overview(&overview)
            })
        }
        ReviewCommand::Add {
            id: review,
            threads,
            revision,
        } => {
            let review = id(review)?;
            let batch: Batch = serde_json::from_slice(&read_input(&threads, "threads")?)
                .context(
                    "threads must be JSON {\"threads\": [...]}; see dots review add --help for the fields",
                )?;
            let added = reviews.add(&review, batch.threads, revision, now)?;
            if json {
                return Ok(crate::json::render(
                    json!({ "review": review, "added": added }),
                ));
            }
            let mut out = format!("Opened {} thread(s) in {review}.\n", added.len());
            for thread in &added {
                render_thread(&mut out, thread);
            }
            Ok(out)
        }
        ReviewCommand::Reply {
            id: review,
            thread,
            author,
            body,
        } => {
            let review = id(review)?;
            let thread = reviews.reply(&review, &thread, author, read_body(body)?, now)?;
            Ok(render_changed(&review, &thread, json))
        }
        ReviewCommand::Set {
            id: review,
            thread,
            status,
            author,
            body,
        } => {
            let review = id(review)?;
            let status =
                Status::parse(&status).context("status parser admitted an unknown status")?;
            let thread = reviews.set(&review, &thread, status, author, body, now)?;
            Ok(render_changed(&review, &thread, json))
        }
        ReviewCommand::Refresh { id } => {
            let refreshed = reviews.refresh(&id, now)?;
            Ok(if json {
                crate::json::render(serde_json::to_value(&refreshed)?)
            } else {
                render_refreshed(&refreshed)
            })
        }
        ReviewCommand::Remove { id } => {
            reviews.remove(&id)?;
            Ok(if json {
                crate::json::render(json!({ "removed": id }))
            } else {
                format!("Removed {id}.\n")
            })
        }
    }
}

fn spec(target: ReviewTarget) -> Result<TargetSpec> {
    Ok(match (target.worktree, target.base, target.range) {
        (true, None, None) => TargetSpec::Worktree,
        (false, Some(reference), None) => TargetSpec::Base { reference },
        (false, None, Some(range)) => TargetSpec::range(&range)?,
        _ => bail!("choose exactly one of --worktree, --base REF or --range FROM..TO"),
    })
}

fn read_body(body: ReviewBody) -> Result<String> {
    match (body.body, body.body_file) {
        (Some(body), None) => Ok(body),
        (None, Some(path)) => String::from_utf8(read_input(&path, "body")?)
            .with_context(|| format!("body in {} is not UTF-8", path.display())),
        _ => bail!("give exactly one of --body TEXT or --body-file PATH"),
    }
}

/// The bytes of `path`, or stdin for `-`, refused past [`INPUT_LIMIT`].
fn read_input(path: &Path, what: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let read = if path == Path::new("-") {
        std::io::stdin()
            .take(INPUT_LIMIT + 1)
            .read_to_end(&mut bytes)
    } else {
        std::fs::File::open(path)
            .and_then(|file| file.take(INPUT_LIMIT + 1).read_to_end(&mut bytes))
    };
    read.with_context(|| format!("read {what} from {}", path.display()))?;
    if bytes.len() as u64 > INPUT_LIMIT {
        bail!("{what} input exceeds {INPUT_LIMIT} bytes");
    }
    Ok(bytes)
}

fn render_changed(review: &str, thread: &Thread, json: bool) -> String {
    if json {
        return crate::json::render(json!({ "review": review, "thread": thread }));
    }
    let mut out = String::new();
    render_thread(&mut out, thread);
    out
}

fn render_list(summaries: &[Summary], all: bool, owner: Option<&str>) -> String {
    if summaries.is_empty() {
        let place = if all {
            "this repository"
        } else {
            "this worktree"
        };
        return match owner {
            Some(owner) => format!("No reviews owned by {owner} in {place}.\n"),
            None => format!("No reviews in {place}.\n"),
        };
    }
    let mut out = String::new();
    for summary in summaries {
        let counts: Vec<String> = summary
            .threads
            .iter()
            .map(|(status, count)| format!("{count} {}", status.name()))
            .collect();
        let place = if all {
            format!(
                "{} · ",
                summary.worktree.branch.clone().unwrap_or_else(|| summary
                    .worktree
                    .path
                    .display()
                    .to_string())
            )
        } else {
            String::new()
        };
        let _ = writeln!(
            out,
            "{} · {place}{} · {}",
            summary.id,
            summary.target,
            if counts.is_empty() {
                "no threads".into()
            } else {
                counts.join(", ")
            }
        );
    }
    out
}

fn render_overview(overview: &Overview) -> String {
    let review = &overview.review;
    let revision = review.current();
    let mut out = format!(
        "{} · {}{}{}\n{}{}\nbase {} · head tree {}{}\n",
        review.id,
        review.target,
        review
            .profile
            .as_deref()
            .map(|profile| format!(" · profile {profile}"))
            .unwrap_or_default(),
        review
            .owner
            .as_deref()
            .map(|owner| format!(" · owner {owner}"))
            .unwrap_or_default(),
        review.worktree.path.display(),
        review
            .worktree
            .branch
            .as_deref()
            .map(|branch| format!(" ({branch})"))
            .unwrap_or_default(),
        revision.base,
        revision.head,
        if revision.includes_worktree {
            " (includes uncommitted work)"
        } else {
            ""
        }
    );
    render_files(&mut out, &overview.files);
    let _ = writeln!(out, "Diff: {}", overview.diff_command);
    if review.threads.is_empty() {
        out.push_str("No threads.\n");
    }
    for thread in &review.threads {
        render_thread(&mut out, thread);
    }
    out
}

fn render_refreshed(refreshed: &Refreshed) -> String {
    let review = &refreshed.review;
    if !refreshed.changed {
        return format!(
            "{} is unchanged at head tree {}.\n",
            review.id,
            review.current().head
        );
    }
    let mut out = format!(
        "{} moved to head tree {}: {} carried, {} outdated.\n",
        review.id,
        review.current().head,
        refreshed.carried.len(),
        refreshed.outdated.len()
    );
    if let Some(delta) = &refreshed.delta {
        out.push_str("Changed since the previous revision:\n");
        render_files(&mut out, &delta.files);
        let _ = writeln!(out, "Delta: {}", delta.diff_command);
    }
    out
}

fn render_files(out: &mut String, files: &[ChangedFile]) {
    for file in files {
        if file.binary {
            let _ = writeln!(out, "  {} (binary)", file.path);
        } else {
            let _ = writeln!(out, "  {} +{} -{}", file.path, file.added, file.removed);
        }
    }
}

fn render_thread(out: &mut String, thread: &Thread) {
    let location = thread.anchor.as_ref().map_or_else(
        || "review".to_owned(),
        |anchor| {
            format!(
                "{}:{}-{}{}",
                anchor.path,
                anchor.start,
                anchor.end,
                if anchor.side == Side::Old {
                    " (old)"
                } else {
                    ""
                }
            )
        },
    );
    let severity = thread.severity.map_or_else(String::new, |severity| {
        format!(
            " [{}{}]",
            severity.name(),
            if thread.pre_existing {
                ", pre-existing"
            } else {
                ""
            }
        )
    });
    let _ = writeln!(
        out,
        "\n{}{severity} {} · {location}",
        thread.id,
        thread.status.name(),
    );
    if let Some(title) = &thread.title {
        let _ = writeln!(out, "{title}");
    }
    for comment in &thread.comments {
        let action = comment
            .status
            .map(|status| format!(" set {}", status.name()))
            .unwrap_or_default();
        let body = comment
            .body
            .as_deref()
            .and_then(|body| body.lines().next())
            .map(|line| format!(": {line}"))
            .unwrap_or_default();
        let _ = writeln!(out, "  {}{action}{body}", comment.author);
    }
}
