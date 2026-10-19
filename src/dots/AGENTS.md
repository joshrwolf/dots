# dots CLI

- Load `idiomatic-rust` for Rust changes. Use the repository-root Cargo workspace.
- Keep explicit library plus thin binary packages. Applications own argument
  parsing, host context, rendering, and exit codes; shared behavior lives with its owner under `src/`. Do not depend on a Herdr plugin crate for domain operations.
- `dots` groups operations by integration (`dots github ci`). Add a subcommand
  only for a concrete workflow; avoid generic API passthrough commands.
- Help must work offline. Commands are noninteractive with bounded output,
  no pager, and explicit follow-up commands when results are partial.
- This CLI is greenfield: change output and reference formats directly without
  version markers or compatibility layers. Separate stdout data from stderr errors and keep failed
  CI distinct from command failure. Validate arguments before provider work.
- Use root `mise run //src/dots:build`, `mise run //src/dots:check`, and `mise run //src/dots:install` for staging,
  validation, and explicit Stow installation. Building must not activate Herdr.

- Command metadata in `src/command.rs` owns parsing, help, and generated
  skill discovery. Put usage/effect guidance there; do not maintain a second
  command list. `mise run //src/dots:skills` updates the checked-in artifacts and the
  validation gates check for drift.
