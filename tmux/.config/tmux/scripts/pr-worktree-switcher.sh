#!/usr/bin/env bash

# Switch to a worktree for an open PR, creating it if needed
# Layout: parent_dir/.bare, parent_dir/main, parent_dir/<branch>

git_dir=$(git rev-parse --git-dir 2>/dev/null)
if [[ $? -ne 0 ]]; then
  exit 0
fi

# Get parent dir (contains .bare, main, and other worktrees)
# e.g., /Users/wolf/dev/gh/chainguard/workies/images-private
parent_dir=$(dirname "$(git worktree list --porcelain | grep '^worktree ' | head -1 | cut -d' ' -f2-)")

# List open PRs authored by me
# Format: number (for preview) | #number | title | branch
pr_list=$(gh pr list --author @me --state open \
  --json number,title,headRefName \
  --jq '.[] | "\(.number)\t#\(.number)\t\(.title)\t\(.headRefName)"' 2>/dev/null)

if [[ -z "$pr_list" ]]; then
  tmux display-message "No open PRs found"
  exit 0
fi

# Pick from list with preview
selected=$(echo "$pr_list" | fzf-tmux -p 80%,60% \
  --layout=reverse \
  --delimiter=$'\t' \
  --with-nth=2,3,4 \
  --preview='gh pr view {1}' \
  --preview-window='right,60%,wrap' \
  --border-label ' my PRs ' \
  --prompt 'PR> ') || exit 0

[[ -n "$selected" ]] || exit 0

# Extract branch name (4th field)
pr_branch=$(echo "$selected" | cut -f4)

# Sanitize branch name for directory (replace / with -)
dir_name="${pr_branch//\//-}"

# Check if worktree already exists for this branch
existing_wt=$(git worktree list --porcelain | grep -A2 "^worktree " | grep -B1 "branch refs/heads/$pr_branch" | grep "^worktree " | cut -d' ' -f2-)

if [[ -n "$existing_wt" ]]; then
  sesh connect "$existing_wt"
  exit 0
fi

# Also check if directory already exists (might be same branch with different ref)
if [[ -d "$parent_dir/$dir_name" ]]; then
  sesh connect "$parent_dir/$dir_name"
  exit 0
fi

# Fetch the branch and create worktree
worktree_path="$parent_dir/$dir_name"
git fetch origin "$pr_branch" 2>/dev/null || true
git worktree add "$worktree_path" "origin/$pr_branch" 2>/dev/null || {
  tmux display-message "Failed to create worktree for $pr_branch"
  exit 1
}

# Track the remote branch
git -C "$worktree_path" checkout -B "$pr_branch" --track "origin/$pr_branch" 2>/dev/null || true

sesh connect "$worktree_path"
