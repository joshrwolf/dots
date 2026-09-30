- Only send messages or replies to other people (on GitHub or elsewhere) when I explicitly ask.
- Prefix AI-generated text in those messages with `:robot:`.
- For workflows supported by `dots`, use its generated `dots` skill and
  `dots --help` to discover the installed commands before assembling equivalent
  shell/API calls. Native tools remain available for unsupported cases.
- Load the `ship-it` skill when I say "ship it" and before creating or updating any PR on my behalf, even when I did not name the skill. "Ship it" authorizes the necessary commits, push, and PR creation or update without another confirmation; it does not authorize merging or monitoring CI.
- Prefer modern Go and proactively migrate older idioms in code being changed,
  including nearby code when the migration is straightforward and preserves
  behavior. Do not keep obsolete patterns just for consistency or ask whether I
  want routine modernization. Use the latest stable Go for new projects; for
  existing projects, check actual support requirements and align module versions,
  CI, and tooling when raising the target. Preserve explicit compatibility and
  behavioral contracts. Use `golang-modernize` for version-sensitive choices.
