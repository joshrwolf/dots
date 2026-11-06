# Skills in dots

Skills live flat in their public or private source repository; names do not
encode ownership. Each has a `SKILL.md` and relative supporting files. Native
mise groups deploy tracked files into `~/.local/share/dots/skills` without
writing private links into public source.

Two directory links expose that namespace: `~/.agents/skills` for Codex/Pi and
`~/.claude/skills` for Claude. Configuration never creates per-agent per-skill
links. Existing file edits follow their source links; new deployment paths
appear on apply/update. A running harness may need a reload or a new session.

## Setup and authoring

Use [machine setup](../bootstrap/README.md); select private source explicitly.
Unmanaged discovery directories stop setup rather than merging unknown files.
Native groups leave unrelated installed content alone; pruning is deliberate.

Create `skills/example/SKILL.md` with `name` and `description` frontmatter,
add its supporting files to Git, then check and apply the selected source.
Keep credentials and machine captures outside both repositories. Generated
host-owned skills, such as Rex's guide, belong to that host's native setup.

Personal Go defaults live in standing instructions and apply without loading a
skill. The `golang` reference and focused `golang-*` skills are adapted from
[samber/cc-skills-golang](https://github.com/samber/cc-skills-golang), maintained
here without upstream sync. `skills/LICENSE-cc-skills-golang` keeps its MIT notice.

To measure whether guidance helps, use
[skill evaluation](../skills/skill-evaluation/SKILL.md).

## Generated CLI skill

`mise run //src/dots:skills` generates the CLI skill/reference.
`dots skills check --output skills/dots` validates them from the repository
root; see [the CLI](../src/dots/README.md).
