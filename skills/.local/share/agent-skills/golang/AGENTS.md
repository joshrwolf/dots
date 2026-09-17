# Go skills maintained in dots

This directory is an editable subtree of samber/cc-skills-golang. Dots owns the
local customizations; the root AGENTS.md and docs/skills.md describe installation
and upstream updates. Preserve the upstream MIT license and attribution.

## Skill structure

- Keep each skill in `skills/<name>/SKILL.md` with `name` and `description`
  frontmatter. The name matches its directory. Preserve useful existing optional
  metadata; new skills need no marketplace-specific metadata.
- Describe the capability and trigger in one or two short sentences. Keep API
  catalogs, workflows, and sibling routing out of descriptions.
- Keep the entrypoint focused on decisions and essential constraints. Put
  conditional detail in relative reference files, linked where it becomes useful.
- Avoid repeated personas, thinking-mode incantations, and duplicate tool guides.
  Keep technical reasoning and task-specific pitfalls.
- Keep instructions shared between agents. Put harness-specific setup in optional
  references and do not assume a tool is installed because a skill mentions it.
- Maintain canonical AGENTS.md files and sibling relative CLAUDE.md symlinks.
- Encourage task-based delegation decisions. Give useful criteria for working
  directly or using subagents; avoid fixed staffing, mandatory fan-out, and blanket
  prohibitions. Keep delegation within the environment's collaboration rules.

## Local tool choices

Use `go doc` for package documentation and gopls for semantic code work. Keep
runtime diagnostics grounded in tests, stack traces, the race detector, pprof,
and execution traces. Use the repository's golangci-lint configuration and keep
GoReleaser for existing release workflows. Avoid adding helper tools as implicit
requirements of ordinary Go tasks. Specialized library skills apply to projects
that use those libraries; they are not blanket dependency recommendations.

## Testing examples

Use ordinary `testing` functions, explicit failure checks, and focused fakes.
`golang-testing` owns test design; `golang-go-cmp` owns structural comparison
semantics and options. Keep examples and evaluation criteria consistent with
those skills. Preserve meaningful failure diagnostics and resource cleanup.

## Review and verification

- Check changed frontmatter, relative links, examples, and evaluation fixtures.
- Compile or run self-contained Go examples when changing their behavior; identify
  illustrative fragments that require application types or external services.
- Check catalog entries and routing when adding or removing a skill. Run
  `make skills-links` from dots, review discovery links, then `make skills`.
- Upstream evaluation reports and size tables describe upstream results, not
  evidence for local edits. Do not invent updated scores or benchmark results.
- Local edits do not require marketplace releases, synchronized plugin versions,
  scanner credentials, or repository-wide formatting. Keep diffs scoped so
  upstream merges remain reviewable.
