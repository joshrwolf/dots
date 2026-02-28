#!/usr/bin/env bash

git_dir=$(git rev-parse --git-dir 2>/dev/null)
if [[ $? -ne 0 ]]; then
  exit 0
fi

# Get parent dir of worktrees (e.g., /Users/wolf/dev/gh/chainguard/workies/images-private)
parent_dir=$(dirname "$(git worktree list --porcelain | grep '^worktree ' | head -1 | cut -d' ' -f2-)")

# Show just names, connect to parent/selected
selected=$(git worktree list --porcelain | grep '^worktree ' | cut -d' ' -f2- | xargs -n1 basename | fzf-tmux -p 40%,30% \
  --no-sort --border-label ' git worktrees ' --prompt ' ')

if [[ -n "$selected" ]]; then
  sesh connect "$parent_dir/$selected"
fi
