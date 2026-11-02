//! `dots review`: parse requests, call the review store, and present results.
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::Path;

use anyhow::{Context as _, Result, bail};
use dots_review::{
    Anchor, ChangedFile, Draft, FileStatus, NewDraft, NewThread, Overview, Preset, Refreshed,
    ReviewedFile, Reviews, ShowOptions, Side, Status, Submitted, Summary, TargetSpec, Thread,
};
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;

use crate::command::{DiscardWhich, ReviewBody, ReviewCommand, ReviewDraft, ReviewTarget};

/// Largest threads file `add`, or body file `reply`, reads.
const INPUT_LIMIT: u64 = 8 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Batch {
    threads: Vec<NewThread>,
}

#[expect(
    clippy::too_many_lines,
    reason = "A flat dispatch with one short arm per subcommand"
)]
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
        ReviewCommand::List { all, owner, drafts } => {
            let mut summaries = reviews.list(all, drafts)?;
            summaries.retain(|summary| owner.is_none() || summary.owner == owner);
            Ok(if json {
                crate::json::render(json!({ "reviews": summaries }))
            } else {
                render_list(&summaries, all, owner.as_deref())
            })
        }
        ReviewCommand::Show {
            id: review,
            drafts,
            preset,
        } => {
            let preset = preset.as_deref().map(preset_of).transpose()?;
            let overview = reviews.show(&id(review)?, ShowOptions { drafts, preset })?;
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
        ReviewCommand::Draft(args) => {
            let review = id(args.id.clone())?;
            record_draft(&reviews, &review, args, now, json)
        }
        ReviewCommand::Discard {
            id: review,
            which: DiscardWhich { draft, all: _ },
        } => {
            let review = id(review)?;
            let discarded = reviews.discard(&review, draft.as_deref())?;
            let ids: Vec<&str> = discarded.iter().map(|draft| draft.id.as_str()).collect();
            Ok(if json {
                crate::json::render(json!({ "review": review, "discarded": ids }))
            } else if ids.is_empty() {
                format!("No drafts in {review}.\n")
            } else {
                format!("Discarded {} from {review}.\n", ids.join(", "))
            })
        }
        ReviewCommand::Submit {
            id: review,
            body,
            body_file,
        } => {
            let review = id(review)?;
            let body = match (body, body_file) {
                (None, None) => None,
                (body, body_file) => Some(read_body(ReviewBody { body, body_file })?),
            };
            let submitted = reviews.submit(&review, body, now)?;
            Ok(if json {
                crate::json::render(json!({
                    "review": review,
                    "submission": submitted.submission,
                    "threads": submitted.threads,
                }))
            } else {
                render_submitted(&review, &submitted)
            })
        }
        ReviewCommand::Mark { id: review, path } => mark(&reviews, &id(review)?, &path, true, json),
        ReviewCommand::Unmark { id: review, path } => {
            mark(&reviews, &id(review)?, &path, false, json)
        }
        ReviewCommand::Look { id: review } => {
            let review = id(review)?;
            let looked = reviews.look(&review, now)?;
            Ok(if json {
                crate::json::render(json!({ "review": review, "looked": looked }))
            } else {
                format!("Looked at revision {} of {review}.\n", looked.revision)
            })
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

fn record_draft(
    reviews: &Reviews,
    review: &str,
    args: ReviewDraft,
    now: i64,
    json: bool,
) -> Result<String> {
    let anchor = args
        .path
        .map(|path| match (args.side.as_deref(), args.start) {
            (Some(side), Some(start)) => Anchor::lines(
                path,
                if side == "old" { Side::Old } else { Side::New },
                start,
                args.end.unwrap_or(start),
            ),
            _ => Anchor::file(path),
        });
    let draft = NewDraft {
        thread: args.thread,
        anchor,
        body: read_body(args.body)?,
    };
    let draft = reviews.draft(review, draft, preset_of(&args.preset)?, args.revision, now)?;
    if json {
        return Ok(crate::json::render(
            json!({ "review": review, "draft": draft }),
        ));
    }
    let mut out = format!("Drafted {} in {review}.\n", draft.id);
    render_draft(&mut out, &draft);
    Ok(out)
}

fn preset_of(name: &str) -> Result<Preset> {
    Preset::parse(name).context("preset parser admitted an unknown preset")
}

fn mark(reviews: &Reviews, review: &str, path: &str, reviewed: bool, json: bool) -> Result<String> {
    let file = reviews.mark(review, path, reviewed)?;
    Ok(if json {
        crate::json::render(json!({ "review": review, "file": file }))
    } else {
        let mut out = String::new();
        render_files(&mut out, std::slice::from_ref(&file));
        out
    })
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
    if !review.drafts.is_empty() {
        let _ = writeln!(out, "\n{} draft(s):", review.drafts.len());
        for draft in &review.drafts {
            render_draft(&mut out, draft);
        }
    }
    if let Some(view) = &overview.view {
        let _ = writeln!(
            out,
            "\nView {}: {}..{} ({} file(s))",
            view.preset.name(),
            view.base,
            view.head,
            view.files.len()
        );
    }
    out
}

fn render_draft(out: &mut String, draft: &Draft) {
    let target = match &draft.thread {
        Some(thread) => format!("reply to {thread}"),
        None => location(draft.anchor.as_ref()),
    };
    let first = draft.body.lines().next().unwrap_or_default();
    let _ = writeln!(out, "  {} · {target}: {first}", draft.id);
}

fn render_submitted(review: &str, submitted: &Submitted) -> String {
    let mut out = format!(
        "Submitted {} comment(s) in {review}.\n",
        submitted.submission.comments.len()
    );
    if let Some(body) = &submitted.submission.body {
        let _ = writeln!(out, "{body}");
    }
    for thread in &submitted.threads {
        render_thread(&mut out, thread);
    }
    out
}

fn location(anchor: Option<&Anchor>) -> String {
    match anchor {
        None => "review".to_owned(),
        Some(Anchor { path, lines: None }) => format!("{path} (file)"),
        Some(Anchor {
            path,
            lines: Some(lines),
        }) => format!(
            "{path}:{}-{}{}",
            lines.start,
            lines.end,
            if lines.side == Side::Old {
                " (old)"
            } else {
                ""
            }
        ),
    }
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
        render_changes(&mut out, &delta.files);
        let _ = writeln!(out, "Delta: {}", delta.diff_command);
    }
    out
}

fn render_files(out: &mut String, files: &[ReviewedFile]) {
    for file in files {
        render_change(out, &file.change);
        if file.reviewed {
            out.push_str(" · reviewed");
        }
        if file.changed_since_looked {
            out.push_str(" · changed since last looked");
        }
        out.push('\n');
    }
}

fn render_changes(out: &mut String, files: &[ChangedFile]) {
    for file in files {
        render_change(out, file);
        out.push('\n');
    }
}

fn render_change(out: &mut String, file: &ChangedFile) {
    let status = match file.status {
        FileStatus::Added => "A",
        FileStatus::Modified => "M",
        FileStatus::Deleted => "D",
        FileStatus::TypeChanged => "T",
    };
    if file.binary {
        let _ = write!(out, "  {status} {} (binary)", file.path);
    } else {
        let _ = write!(
            out,
            "  {status} {} +{} -{}",
            file.path, file.added, file.removed
        );
    }
}

fn render_thread(out: &mut String, thread: &Thread) {
    let location = location(thread.anchor.as_ref());
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
