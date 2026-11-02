# Documentation

## Using the Plugin

- **[git-integration.md](git-integration.md)** — Compare files with any git revision (`:CodeDiff HEAD`, `:CodeDiff main`, etc.)
- **[performance.md](performance.md)** — Timeout control for large files and how the two-phase diff balances speed vs. detail

## Building & Contributing

- **[BUILD.md](BUILD.md)** — Build system guide: CMake, Makefile targets, standalone scripts for users without CMake
- **[VERSION_MANAGEMENT.md](VERSION_MANAGEMENT.md)** — Semantic versioning workflow and automated version bumping
- **[dependency-distribution.md](dependency-distribution.md)** — How OpenMP/libgomp is bundled so users don't need system packages

## Algorithm Internals

- **[filler-line-algorithm.md](filler-line-algorithm.md)** — How filler lines align side-by-side views, matching VSCode's `computeRangeAlignment()`
- **[DIFF_NOTATION.md](DIFF_NOTATION.md)** — Line-level (`seq1[start,end)`) and character-level (`L:C`) notation reference
- **[rendering-quick-reference.md](rendering-quick-reference.md)** — The 3-step rendering process: line highlights → char highlights → filler lines

## Development History

For the full story of building codediff.nvim — algorithm development, parity evaluations, architecture decisions:

- **[development/](development/)** — 14 months of development logs organized into narrative reading paths
