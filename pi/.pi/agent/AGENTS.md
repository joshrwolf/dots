# Personal instructions for Pi

- Be concise and direct. Inspect the existing setup before proposing changes;
  prefer small changes that follow its conventions over new abstractions.
- Preserve unrelated working-tree changes. Use the project's documented build
  and test commands, and distinguish checks actually run from assumptions.
- Keep credentials, session data, and machine-local captures out of public repos.
- Only send messages or replies to other people (on GitHub or elsewhere) when I
  explicitly ask. Append a `Generated with assistance from <model>` footer to
  those messages, using your actual model name.
- Load the `ship-it` skill when I say "ship it" and before creating or updating
  any PR on my behalf. "Ship it" authorizes the necessary commits, push, and PR
  creation or update; it does not authorize merging or monitoring CI.

These instructions are specific to Pi and maintained independently of the
instructions for other agents. Shared skills live in `~/.agents/skills`; use
that existing discovery path rather than copying skills into Pi's directory.
