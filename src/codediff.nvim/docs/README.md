# Documentation

## Using the Plugin

- **[git-integration.md](git-integration.md)** — Compare files with any git revision (`:CodeDiff HEAD`, `:CodeDiff main`, etc.)
- **[performance.md](performance.md)** — Timeout control for large files and how the two-phase diff balances speed vs. detail

## Algorithm Internals

- **[filler-line-algorithm.md](filler-line-algorithm.md)** — How filler lines align side-by-side views, matching VSCode's `computeRangeAlignment()`
- **[DIFF_NOTATION.md](DIFF_NOTATION.md)** — Line-level (`seq1[start,end)`) and character-level (`L:C`) notation reference
- **[rendering-quick-reference.md](rendering-quick-reference.md)** — The 3-step rendering process: line highlights → char highlights → filler lines
