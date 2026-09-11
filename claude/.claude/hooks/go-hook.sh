#!/bin/bash
INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path // empty')
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')

[[ -z "$FILE_PATH" || "$FILE_PATH" != *.go || ! -f "$FILE_PATH" ]] && exit 0
[[ -z "$SESSION_ID" ]] && exit 0

echo "$FILE_PATH" >> "/tmp/go-pending-${SESSION_ID}"

FILENAME=$(basename "$FILE_PATH")

SYNTAX_ERR=$(gofmt -e "$FILE_PATH" 2>&1 >/dev/null)
if [[ -n "$SYNTAX_ERR" ]]; then
  echo "$SYNTAX_ERR" | sed "s|$FILE_PATH:|$FILENAME:|g" >&2
  exit 2
fi

gofmt -w "$FILE_PATH" 2>/dev/null

exit 0
