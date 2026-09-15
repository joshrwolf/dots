use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, Result, ensure};
use herdrkit::Client;

use crate::{Issue, store::Store};

pub(crate) fn command(client: &Client, state: &Path, cwd: &Path, kind: &str) -> Result<Command> {
    ensure!(
        ["codex", "claude"].contains(&kind),
        "unsupported issue agent"
    );
    let listing = client
        .worktree_list(cwd)
        .context("resolving current checkout")?;
    let cwd = std::fs::canonicalize(cwd).context("resolving current directory")?;
    // Herdr's source_checkout_path can name the repository's primary checkout,
    // even when cwd is in a linked worktree. Resolve the containing checkout
    // from the worktree paths instead, preferring the nearest ancestor.
    let tree = listing
        .worktrees
        .iter()
        .filter(|tree| cwd.starts_with(&tree.checkout_path))
        .max_by_key(|tree| tree.checkout_path.components().count())
        .context("current directory is not inside a listed checkout")?;
    ensure!(
        !tree.is_detached && !tree.is_bare && !tree.is_prunable,
        "current checkout must be an available branch checkout"
    );
    let checkout = &tree.checkout_path;
    let branch = tree
        .branch
        .as_deref()
        .context("current checkout has no branch")?;
    let root = std::fs::canonicalize(&listing.source.repository.root)
        .context("resolving repository root")?;
    let issue = Store::open(state)?.context(&root, branch)?;
    let mut command = Command::new(kind);
    command.current_dir(checkout).arg("--").arg(prompt(&issue)?);
    Ok(command)
}

fn prompt(issue: &Issue) -> Result<String> {
    let context = issue
        .prompt
        .as_deref()
        .filter(|text| !text.trim().is_empty())
        .context("this issue has no saved prompt; reopen it from Linear to supply its context")?;
    let introduction = format!(
        "Work on Linear issue {} in this existing checkout (branch {}). Follow the repository instructions.\n\n",
        issue.identifier, issue.branch
    );
    // Keep the initial argv comfortably below platform argument-size limits.
    // Large handoffs remain intact in the local context store.
    if context.len() > 32 * 1024 {
        return Ok(format!(
            "{introduction}The full issue prompt is saved locally. Before starting work, run herdr-issue context with --repo set to this checkout and --branch set to {}. Read the complete prompt field and use it as the task instructions. Use your existing Linear MCP for fresh details if needed.",
            issue.branch
        ));
    }
    Ok(format!("{introduction}Saved issue prompt:\n\n{context}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_preserves_issue_content_as_data() {
        let text = "Fix café 👨‍👩‍👧. Show `$(example)` literally.";
        let issue = Issue::new("ENG-12".into(), "eng-12".into(), None, Some(text.into())).unwrap();
        let output = prompt(&issue).unwrap();
        assert!(output.contains(text));
        assert!(output.contains("ENG-12"));
    }

    #[test]
    fn missing_context_fails_and_large_context_is_retrieved_without_truncation() {
        let mut issue = Issue::new("ENG-12".into(), "eng-12".into(), None, None).unwrap();
        assert!(prompt(&issue).is_err());
        issue.prompt = Some("x".repeat(64 * 1024));
        let output = prompt(&issue).unwrap();
        assert!(output.len() < 1024);
        assert!(output.contains("herdr-issue context"));
    }
}
