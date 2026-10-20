# dots CLI

- Load `idiomatic-rust` for Rust changes. Use the repository-root Cargo workspace.
- Keep explicit library plus thin binary packages. Applications own argument
  parsing, host context, rendering, and exit codes; shared behavior lives with its
  owner under `src/`. Do not depend on a Herdr plugin crate for domain operations.
- `dots` groups operations by integration (`dots github ci`). Add a subcommand
  only for a concrete workflow; avoid generic API passthrough commands.
- Help must work offline. Commands are noninteractive with bounded output,
  no pager, and explicit follow-up commands when results are partial.
- Output/reference formats have no compatibility guarantee; change them directly
  rather than adding version markers or compatibility layers. Separate stdout
  data from stderr errors. Failed CI is not command failure; validate arguments
  before provider work.
- Use `mise run //src/dots:check` for validation and
  `mise run //src/dots:install` (`cargo install`) for explicit installation.
- Command metadata in `src/command.rs` owns parsing, help, and generated
  skill discovery. Put usage/effect guidance there; do not maintain a second
  command list. `mise run //src/dots:skills` updates the checked-in artifacts and the
  validation gates check for drift.
