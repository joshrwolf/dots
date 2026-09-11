# Neovim configuration

- Optimize for a small, easy-to-maintain personal configuration.
- Prefer built-in capabilities and documented APIs. Avoid depending on newly
  introduced experimental behavior just because the installed version supports it.
- Use plugin defaults unless a concrete need justifies an override. Add
  dependencies deliberately and avoid overlapping functionality.
- Keep setup explicit and easy to follow. Introduce modules when they improve
  clarity; avoid custom frameworks, automatic discovery, and speculative abstractions.
- Consult upstream documentation and established configurations for guidance,
  adapting only what this configuration needs.
- Give mappings clear descriptions and check for conflicts with existing
  editor and terminal behavior.
- Keep the README a stable overview. Put specific explanations near the relevant
  configuration, focusing comments on intent rather than restating the code.
- Verify startup and the behavior affected by a change. Keep validation
  proportional to the change.

## Preferences

- Keep find/search windows and picker mappings under `<leader>f` (Space is leader).
- Prefer paired `]` / `[` mappings for next/previous navigation, such as
  `]t` / `[t` for tabs and `]h` / `[h` for Git hunks.
