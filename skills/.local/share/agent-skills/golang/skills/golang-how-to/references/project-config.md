# Project skill guidance

Use this procedure when the user requests project skill configuration. Creating a
Go project or consulting a skill does not itself call for editing agent instructions.

1. Read existing project instructions and use the requested scope and file. Follow
   the repository's canonical instruction-file convention. In this dots setup,
   edit `AGENTS.md` and use a sibling relative `CLAUDE.md -> AGENTS.md` symlink.
   Do not duplicate the guidance across every detected harness configuration.
2. Use preferences already provided in the session. Ask only when an unresolved
   choice materially changes the configuration; do not re-confirm an agreed list.
3. Add concise, task-specific guidance. Prefer conditional references over an
   always-loaded list. Preserve unrelated instructions and avoid duplicate blocks.
4. Check referenced skills are installed under the names the project uses. Report
   the file changed and the situations in which the guidance applies.

For example, a project that frequently changes concurrent code might use:

```markdown
## Go development

Follow existing package conventions. Consult `golang-concurrency` when changing
goroutine lifetimes or synchronization, and `golang-context` when changing
cancellation or deadlines. Use `golang-testing` when designing tests.
```

Do not turn this example into a default bundle. Include only guidance relevant to
the requested project. If the user explicitly wants mandatory loading, honor that
scope instead of extending it to other skills or projects.

For a requested Cursor integration, [cursor-go-skills.mdc](../assets/cursor-go-skills.mdc)
is an optional template. Keep it conditional and preserve unrelated rules. Tool
installation, global agent settings, and configuration for additional harnesses
are separate tasks, not prerequisites for adding project guidance.
