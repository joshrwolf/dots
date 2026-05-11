## Communication Style

When asked for honest architectural feedback, give it directly. Do not defend
existing implementations by default.

## GitHub Interactions

NEVER post comments, replies, or reviews on GitHub PRs/issues on my behalf.
Always show me the proposed response and let me post it myself.

## Go code

For ANY Go work (reading, editing, refactoring .go files, navigating Go code,
running Go tooling), you MUST load the `gotools` skill before proceeding. It
covers the LSP tool for navigation, gopls CLI for rename/format/imports, and
the diagnostics hook. Do not skip this — it is not optional.

## Comments in code

Default to no comments. The bar for adding one is high: a hidden constraint,
a non-obvious invariant, a workaround for a specific bug, or behavior that
would surprise a careful reader. If a comment restates what the code already
says, delete it.

Exception: write doc comments on **exported / public** APIs where the
language convention or linter requires them — Go exported identifiers
(`// FuncName does X.`), Rust `///` on `pub` items, Python docstrings on
public modules/classes/functions, TSDoc on package-public exports. Keep
these tight and factual; do not pad them.

Concretely, do NOT write any of these (even on public APIs):
- Section headers ("// --- helpers ---", "# Setup")
- Restatements ("// increment counter" above `i++`)
- Docstrings on internal/unexported functions whose name and signature are
  self-evident
- "Added for X" / "Used by Y" / "Removed Z" — that belongs in commit messages
- TODOs without an owner and a concrete next action
- Parameter descriptions that duplicate the type signature

When in doubt, no comment. I will ask if I want one.

## Subagents

Do not spawn a subagent for work you can complete directly in a single
response (e.g., reading one file, editing a function you can already see,
running one grep). Spawn multiple subagents in the same turn when fanning
out across independent items. Prefer foreground over background unless I
have other work to do while waiting.
# graphify
- **graphify** (`~/.claude/skills/graphify/SKILL.md`) - any input to knowledge graph. Trigger: `/graphify`
When the user types `/graphify`, invoke the Skill tool with `skill: "graphify"` before doing anything else.
