#!/usr/bin/env bash

workspace_root=$(jj workspace root 2>/dev/null)
if [ $? -eq 0 ]; then
  parent_dir=$(dirname "$workspace_root")
  selected=$(jj workspace list -T 'self.name() ++ "\n"' | fzf-tmux -p 40%,30% \
    --no-sort --border-label ' jj workspaces ' --prompt '🥋  ')

  if [ -n "$selected" ]; then
    sesh connect "$parent_dir/$selected"
  fi
fi
