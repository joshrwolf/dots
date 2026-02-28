#!/usr/bin/env bash

pane_path="$1"

cd "$pane_path" 2>/dev/null || exit 0

# Check if we're in a jj workspace and use starship-jj
if jj workspace root >/dev/null 2>&1; then
  starship-jj --ignore-working-copy starship prompt 2>/dev/null | \
    sed -E 's/\x1b\[31;49m/#[fg=red]/g' | \
    sed -E 's/\x1b\[32;49m/#[fg=green]/g' | \
    sed -E 's/\x1b\[33;49m/#[fg=yellow]/g' | \
    sed -E 's/\x1b\[34;49m/#[fg=blue]/g' | \
    sed -E 's/\x1b\[35;49m/#[fg=magenta]/g' | \
    sed -E 's/\x1b\[36;49m/#[fg=cyan]/g' | \
    sed -E 's/\x1b\[37;49m/#[fg=white]/g' | \
    sed -E 's/\x1b\[39;49m/#[default]/g' | \
    sed -E 's/\x1b\[0m/#[default]/g'
fi
