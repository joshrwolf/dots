# Personal instructions for Pi

- Be concise and direct. Inspect the existing setup before proposing changes;
  prefer small changes that follow its conventions over new abstractions.
- Preserve unrelated working-tree changes. Use the project's documented build
  and test commands, and distinguish checks actually run from assumptions.
- Keep READMEs concise and user-facing. Put agent-specific guidance in the
  relevant `AGENTS.md`, not historical debugging notes or session reports.
- Keep credentials, session data, and machine-local captures out of public repos.
- For workflows supported by `dots`, use its generated `dots` skill and
  `dots --help` to discover the installed commands before assembling equivalent
  shell/API calls. Native tools remain available for unsupported cases.
- Only send messages or replies to other people (on GitHub or elsewhere) when I
  explicitly ask. Prefix AI-generated text in those messages with `:robot:`.
- Load the `ship-it` skill when I say "ship it" and before creating or updating
  any PR on my behalf. "Ship it" authorizes the necessary commits, push, and PR
  creation or update; it does not authorize merging or monitoring CI.
- Prefer modern Go and proactively migrate older idioms in code being changed,
  including nearby code when the migration is straightforward and preserves
  behavior. Do not keep obsolete patterns just for consistency or ask whether I
  want routine modernization. Use the latest stable Go for new projects; for
  existing projects, check actual support requirements and align module versions,
  CI, and tooling when raising the target. Preserve explicit compatibility and
  behavioral contracts. Use `golang-modernize` for version-sensitive choices.

These instructions are specific to Pi and maintained independently of the
instructions for other agents. Shared skills live in `~/.agents/skills`; use
that existing discovery path rather than copying skills into Pi's directory.
