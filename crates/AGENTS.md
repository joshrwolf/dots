# Shared Rust libraries

- Load `idiomatic-rust` for Rust changes. Root `Cargo.toml`, `Cargo.lock`,
  `clippy.toml`, and `rust-toolchain.toml` govern every Rust package.
- Provider libraries own external command/API boundaries and validated identities.
  Domain libraries own operations and results. Neither prints user output,
  dispatches harness actions, or requires a Herdr session.
- Pass context explicitly. Resolve working directories and environment in the
  application or host adapter, not implicitly in domain code.
- Expose typed results that CLI, Herdr, and future Pi/MCP adapters can share.
  Add abstractions for demonstrated reuse, not speculative provider frameworks.
- Keep query failures distinct from absent or successful results. Bound external
  calls and output. References must identify the inspected evidence, not a moving
  branch or whichever run is newest.
- Use neutral hand-authored fixtures. Never save live machine/API output here.
- `make cli-check` checks the standalone dependency graph; `make rust-check`
  checks the complete workspace, including Herdr adapters and contract tests.
