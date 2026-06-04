#!/bin/bash
INPUT=$(cat)
SESSION_ID=$(echo "$INPUT" | jq -r '.session_id // empty')
[[ -z "$SESSION_ID" ]] && exit 0

PENDING="/tmp/go-pending-${SESSION_ID}"
[[ ! -f "$PENDING" ]] && exit 0

STOP_HOOK_ACTIVE=$(echo "$INPUT" | jq -r '.stop_hook_active // false')

FILES=$(sort -u "$PENDING")
rm -f "$PENDING"

find_go_root() {
  local dir="$1" mod_dir=""
  while [[ "$dir" != "/" ]]; do
    [[ -z "$mod_dir" && -f "$dir/go.mod" ]] && mod_dir="$dir"
    [[ -f "$dir/go.work" ]] && { echo "$dir"; return; }
    dir="$(dirname "$dir")"
  done
  echo "${mod_dir:-}"
}

BATCH_DIR="/tmp/go-stop-${SESSION_ID}"
mkdir -p "$BATCH_DIR"
trap 'rm -rf "$BATCH_DIR"' EXIT

while IFS= read -r FILE_PATH; do
  [[ -z "$FILE_PATH" || ! -f "$FILE_PATH" ]] && continue
  ROOT=$(find_go_root "$(dirname "$FILE_PATH")")
  [[ -z "$ROOT" ]] && continue

  ROOT_HASH=$(printf '%s' "$ROOT" | shasum | cut -d' ' -f1)
  printf '%s\n' "$ROOT" > "$BATCH_DIR/${ROOT_HASH}.root"

  FILE_REL="${FILE_PATH#$ROOT/}"
  printf '%s\n' "./$(dirname "$FILE_REL")" >> "$BATCH_DIR/${ROOT_HASH}.pkgs"
  printf '%s\n' "$FILE_REL" >> "$BATCH_DIR/${ROOT_HASH}.relpaths"
done <<< "$FILES"

for ROOT_FILE in "$BATCH_DIR"/*.root; do
  [[ ! -f "$ROOT_FILE" ]] && continue
  HASH="${ROOT_FILE%.root}"; HASH="${HASH##*/}"
  sort -u "$BATCH_DIR/${HASH}.pkgs" > "$BATCH_DIR/${HASH}.pkgs.u"
  sort -u "$BATCH_DIR/${HASH}.relpaths" > "$BATCH_DIR/${HASH}.relpaths.u"
done

filter_to_edited() {
  local relpaths_file="$1"
  while IFS= read -r line; do
    while IFS= read -r rel; do
      case "$line" in
        *"$rel:"*) echo "$line"; break ;;
      esac
    done < "$relpaths_file"
  done
}

run_vet() {
  local root="$1" hash="$2"
  local args=()
  while IFS= read -r pkg; do
    [[ -n "$pkg" ]] && args+=("$pkg")
  done < "$BATCH_DIR/${hash}.pkgs.u"
  [[ ${#args[@]} -eq 0 ]] && return

  local diag
  diag=$(cd "$root" && timeout 30 go vet "${args[@]}" 2>&1) || true
  [[ -z "$diag" ]] && return
  printf '%s\n' "$diag" \
    | grep -v '^# ' \
    | sed -E 's/^vet: //' \
    | filter_to_edited "$BATCH_DIR/${hash}.relpaths.u" \
    | sed -E 's|^.*/([^/]+:[0-9])|\1|'
}

run_gopls() {
  local root="$1" hash="$2"
  local file_args=()
  while IFS= read -r f; do
    [[ -n "$f" ]] && file_args+=("$f")
  done < "$BATCH_DIR/${hash}.relpaths.u"
  [[ ${#file_args[@]} -eq 0 ]] && return

  local diag
  diag=$(cd "$root" && timeout 60 gopls check "${file_args[@]}" 2>/dev/null) || true
  [[ -z "$diag" ]] && return
  printf '%s\n' "$diag" \
    | filter_to_edited "$BATCH_DIR/${hash}.relpaths.u" \
    | sed -E 's|^.*/([^/]+:[0-9])|\1|'
}

VET_OUTPUT=""
for ROOT_FILE in "$BATCH_DIR"/*.root; do
  [[ ! -f "$ROOT_FILE" ]] && continue
  ROOT=$(cat "$ROOT_FILE")
  HASH="${ROOT_FILE%.root}"; HASH="${HASH##*/}"

  RESULT=$(run_vet "$ROOT" "$HASH")
  [[ -n "$RESULT" ]] && { [[ -n "$VET_OUTPUT" ]] && VET_OUTPUT+=$'\n'; VET_OUTPUT+="$RESULT"; }
done

if [[ -n "$VET_OUTPUT" ]]; then
  echo "$VET_OUTPUT" >&2
  exit 2
fi

[[ "$STOP_HOOK_ACTIVE" == "true" ]] && exit 0
command -v gopls >/dev/null || exit 0

GOPLS_OUTPUT=""
for ROOT_FILE in "$BATCH_DIR"/*.root; do
  [[ ! -f "$ROOT_FILE" ]] && continue
  ROOT=$(cat "$ROOT_FILE")
  HASH="${ROOT_FILE%.root}"; HASH="${HASH##*/}"

  RESULT=$(run_gopls "$ROOT" "$HASH")
  [[ -n "$RESULT" ]] && { [[ -n "$GOPLS_OUTPUT" ]] && GOPLS_OUTPUT+=$'\n'; GOPLS_OUTPUT+="$RESULT"; }
done

[[ -z "$GOPLS_OUTPUT" ]] && exit 0
echo "$GOPLS_OUTPUT" >&2
exit 2
