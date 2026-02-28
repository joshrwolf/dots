#!/usr/bin/env bash

pane_path="$1"
session_name="$2"

cd "$pane_path" 2>/dev/null || { echo "$session_name"; exit 0; }

# Check if we're in a jj workspace
if jj workspace root >/dev/null 2>&1; then
  workspace_root=$(jj workspace root 2>/dev/null)
  workspace_name=$(basename "$workspace_root")
  parent_dir=$(dirname "$workspace_root")
  repo_name=$(basename "$parent_dir")

  echo "$repo_name/$workspace_name"
else
  echo "$session_name"
fi
