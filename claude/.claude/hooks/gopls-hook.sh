#!/bin/bash
# Lightweight: just track which .go files were edited this turn.
# Diagnostics run once at Stop, not per-edit.
INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
[[ -z "$FILE_PATH" || "$FILE_PATH" != *.go || ! -f "$FILE_PATH" ]] && exit 0
[[ -z "$SESSION_ID" ]] && exit 0
echo "$FILE_PATH" >> "/tmp/gopls-pending-${SESSION_ID}"
exit 0
