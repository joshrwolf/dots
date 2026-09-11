#!/bin/bash
INPUT=$(cat)
TOOL=$(echo "$INPUT" | jq -r '.tool_name // empty')

deny() {
  jq -n --arg r "$1" '{hookSpecificOutput:{hookEventName:"PreToolUse",permissionDecision:"deny",permissionDecisionReason:$r}}'
  exit 0
}

case "$TOOL" in
  Grep|Glob)
    PATH_ARG=$(echo "$INPUT" | jq -r '.tool_input.path // empty')
    [[ "$PATH_ARG" == "/" ]] && deny "Searching the filesystem root (/) is forbidden — it scans the whole disk. Set path to the project directory or a specific subtree."
    ;;
  Bash)
    CMD=$(echo "$INPUT" | jq -r '.tool_input.command // empty')
    if echo "$CMD" | grep -Eq '(^|[|&;[:space:]])(find|rg|grep|fd|ag)([[:space:]][^|&;]*)?[[:space:]]/([[:space:]]|$)'; then
      deny "This command searches the filesystem root (/). Scope it to the project directory or a specific subtree instead."
    fi
    ;;
esac

exit 0
