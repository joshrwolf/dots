# GitHub libraries

- Provider libraries own external command/API boundaries and validated identities.
  Domain libraries own operations and results. Neither prints user output,
  dispatches harness actions.
- Pass context explicitly. Resolve working directories and environment in the
  application or host adapter, not implicitly in domain code.
- Expose typed results independently of the consuming frontend.
  Add abstractions for demonstrated reuse, not speculative provider frameworks.
- Keep query failures distinct from absent or successful results. Bound external
  calls and output. References must identify the inspected evidence, not a moving
  branch or whichever run is newest.
- Use neutral hand-authored fixtures. Never save live machine/API output here.
- `cargo nextest run -p <crate>` checks one crate; root `mise run check`
  checks the complete workspace.
