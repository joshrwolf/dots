# dots-diff

`src/lib.rs` owns the viewer; `src/main.rs` only parses the user command
line. Keep the library's public surface to what `main.rs` and the examples
call.

Development tools are examples behind the `dev` feature, run from the
checkout and never installed:

- `cargo run --release --example bench --features dev -- PATH [--worktree WT]...`
  times loading, diffing, highlighting and drawing in-process against real
  worktrees, and reports memory. Add `--summaries [--cold]` for the
  switcher's per-worktree counts. It is read-only and starts no Git
  daemons; compare numbers before and after a change on the same machine.
- `cargo run --release --example render --features dev -- PATH --png FILE`
  draws one frame as text, and as a PNG when asked, without a terminal.
  Frame assertions in tests use the library directly.

Tests that need a pseudo-terminal or a disposable Rex server or Pi live in
`tests/acceptance.rs` and run under `verify`.
