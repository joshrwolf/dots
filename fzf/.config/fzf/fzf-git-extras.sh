#!/usr/bin/env bash
# Custom fzf-git extensions - source after fzf-git.sh

# PR worktree switcher: list open PRs, checkout to worktree, open in sesh
# Usage: CTRL-G CTRL-P or call gpr directly
gpr() {
  _fzf_git_check || return

  # Get parent dir of worktrees (contains .bare, main, etc.)
  local parent_dir=$(dirname "$(git worktree list --porcelain | grep '^worktree ' | head -1 | cut -d' ' -f2-)")

  # List open PRs: number, title, branch
  local pr_list=$(gh pr list --author @me --state open \
    --json number,title,headRefName \
    --jq '.[] | "\(.number)\t#\(.number)\t\(.title)\t\(.headRefName)"' 2>/dev/null)

  if [[ -z "$pr_list" ]]; then
    echo "No open PRs found" >&2
    return 1
  fi

  local selected=$(echo "$pr_list" | _fzf_git_fzf --ansi \
    --layout=reverse \
    --delimiter=$'\t' \
    --with-nth=2,3,4 \
    --preview='gh pr view {1}' \
    --preview-window='right,60%,wrap' \
    --border-label=' PRs ' \
    --prompt='PR> ' \
    --header=$'ENTER: checkout to worktree + open session\n') || return

  [[ -z "$selected" ]] && return

  local pr_branch=$(echo "$selected" | cut -f4)
  local dir_name="${pr_branch//\//-}"
  local worktree_path=""

  # Check if worktree already exists for this branch
  worktree_path=$(git worktree list --porcelain | grep -A2 "^worktree " | grep -B1 "branch refs/heads/$pr_branch" | grep "^worktree " | cut -d' ' -f2-)

  # Check if directory exists
  if [[ -z "$worktree_path" && -d "$parent_dir/$dir_name" ]]; then
    worktree_path="$parent_dir/$dir_name"
  fi

  # Create new worktree if needed
  if [[ -z "$worktree_path" ]]; then
    worktree_path="$parent_dir/$dir_name"
    git fetch origin "$pr_branch" >/dev/null 2>&1 || true
    if ! git worktree add "$worktree_path" "origin/$pr_branch" >/dev/null 2>&1; then
      echo "Failed to create worktree for $pr_branch" >&2
      return 1
    fi
    git -C "$worktree_path" checkout -B "$pr_branch" --track "origin/$pr_branch" >/dev/null 2>&1 || true
  fi

  sesh connect "$worktree_path"
}

# Bind CTRL-G CTRL-P for PRs
if [[ -n "${BASH_VERSION:-}" ]]; then
  bind -x '"\C-g\C-p": gpr'
elif [[ -n "${ZSH_VERSION:-}" ]]; then
  _fzf_git_prs-widget() {
    gpr
    zle reset-prompt
  }
  zle -N _fzf_git_prs-widget
  bindkey '^g^p' _fzf_git_prs-widget
fi
