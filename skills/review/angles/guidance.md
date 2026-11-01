# Angle: guidance

Name: `guidance`

Does the change follow the repository's own written rules?

- Find the instruction files that govern the changed paths: `AGENTS.md` (and
  `CLAUDE.md` if present) at the root and in every directory between the root
  and each changed file, plus documents they link as binding (style guides,
  contributor rules).
- Flag only a clear violation of a specific rule, and cite it with `rule`
  (path and line range in the reviewed head). A rule's spirit or your own
  preferences are not findings.
- Check that docs, help text, comments and tests the rules require were
  updated with the behavior, and that the change leaves nothing the rules
  forbid (dead paths, compatibility shims, stale references).
- When rules conflict, or a rule plainly does not fit the case, say so in your
  final message instead of recording a finding.
