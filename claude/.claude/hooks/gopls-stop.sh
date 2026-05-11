#!/bin/bash
# Run gopls diagnostics once per turn on all .go files edited during the turn.
INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
[[ -z "$SESSION_ID" ]] && exit 0

PENDING="/tmp/gopls-pending-${SESSION_ID}"
[[ ! -f "$PENDING" ]] && exit 0

command -v gopls >/dev/null || { rm -f "$PENDING"; exit 0; }

FILES=$(sort -u "$PENDING")
rm -f "$PENDING"

OUTPUT=""
while IFS= read -r FILE_PATH; do
  [[ -z "$FILE_PATH" || ! -f "$FILE_PATH" ]] && continue

  MODULE_DIR="$(dirname "$FILE_PATH")"
  while [[ "$MODULE_DIR" != "/" && ! -f "$MODULE_DIR/go.mod" ]]; do
    MODULE_DIR="$(dirname "$MODULE_DIR")"
  done
  [[ ! -f "$MODULE_DIR/go.mod" ]] && MODULE_DIR="$(dirname "$FILE_PATH")"

  REL_PATH="${FILE_PATH#$MODULE_DIR/}"
  FILENAME="$(basename "$FILE_PATH")"
  DIAG=$(cd "$MODULE_DIR" && gopls check -severity=hint "$REL_PATH" 2>&1)

  if [[ -n "$DIAG" ]]; then
    FORMATTED=$(echo "$DIAG" | sed -E "s|^$FILE_PATH:|$FILENAME:|")
    [[ -n "$OUTPUT" ]] && OUTPUT+=$'\n'
    OUTPUT+="$FORMATTED"
  fi
done <<< "$FILES"

[[ -z "$OUTPUT" ]] && exit 0
echo "$OUTPUT" >&2
exit 2
