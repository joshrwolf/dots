# Sources and syncing

This skill follows the diagram guidance Anthropic ships with Claude's artifact
features. That guidance changes often, so re-check these sources when
updating the skill: compare each source with the version recorded here, carry
over what changed and still applies, and update the recorded versions.

## Sources

| Source | Version used | Where to look |
|---|---|---|
| Claude artifact type skills (Design, Slides, Docs, Dashboard), unofficial copies | commit `e7e53f2b` (2026-10-19), captured upstream 2026-10-10 | `github.com/asgeirtj/system_prompts_leaks`, `Anthropic/artifact-types/{design,slides}/artifact-type/reference/diagrams.md` and `slides/…/diagram-recipes.md` |
| Claude Code bundled `dataviz` skill | Claude Code 2.1.306 | Unpacked on use under `$TMPDIR/claude-<uid>/bundled-skills/<version>/<hash>/dataviz/references/`. The SKILL.md body is not on disk; ask a Claude Code session to load `dataviz` and summarise it. |
| Claude Code artifact tools (`Artifact`, `ArtifactCheck`, …) | not inspected | Their type guidance is fetched from the server per artifact. They need artifacts enabled for the account; when they are, ask a Claude Code session to read an artifact type and report its instructions. |
| claude.ai inline visuals (`visualize` read_me modules) | not used | Older than the artifact type skills; check whether a newer capture exists before relying on it. |

The artifact skill copies are unofficial and proprietary. Take their rules
and reasoning, written in our own words; copy no long passages.

## What was taken, and why

From the artifact diagram guidance:

- Boxes first: settle every box on a regular grid before any connector, then
  derive connectors from box edges. The kit does this structurally: nodes
  have cells, edges have no coordinates.
- Box size comes from the longest label; when space runs out, use fewer
  columns, wrap a chain or merge boxes, never smaller text.
- Connectors run from edge midpoint to edge midpoint in horizontal and
  vertical segments through the gaps between boxes, never across a box; two
  connectors between the same pair use separate points on the edge; a loop
  back goes around the outside. The kit's router and spreading implement this.
- Labels stay clear of boxes, other labels and other edges; gaps widen to fit
  them. One deliberate difference: the artifact guidance always puts a label
  beside its line, at least 8px clear. Here a label sits on its own line over
  a background patch when that is clear of everything else, and moves beside
  the line only when it is not (for example on parallel edges). On a static
  page the patch reads clearly and saves space; beside-the-line is the
  fallback that keeps parallel edges unambiguous.
- Split at about 12 nodes. Per-layout idioms for trees (levels as rows, leaves
  set the columns, parent over children, a bus join), swimlanes (hand-offs stay
  in the same column) and decisions (answers in the child box).
- The words carry the structure (numbered steps, a title that states what the
  diagram shows), for readers and for accessibility. The kit also generates a
  text version of every diagram and makes clickable boxes real buttons.
- Never invent statistics or quotes; extend an existing page in its own
  vocabulary and change only what was asked (artifact craft guidance).
- Snake a long chain into a second row running back, with a turn edge and
  numbered steps.
- The geometry is the check: verify from the layout, not from screenshots.

From the inline-visuals guidance (older, partly superseded): flat, sentence
case, two text sizes in diagrams, at most a few colour categories with a
legend, short subtitles, explanation outside the visual, text kept inside the
viewBox, wide scripts measured wider, and a separate illustrative form for
mechanisms where the geometry is the explanation.

From `dataviz`: form before colour, the job-to-chart table, colour roles
(including one colour for a single nominal series), mark sizes, shared
scales, labelling, a data table for values, and anti-patterns (see
[charts](charts.md)).

## After a sync

Have an independent reviewer compare the updated skill with these sources and
probe the kit in Node (empty and malformed specs, parallel and self-loop
edges, long labels and notes, wide scripts, groups, trees, swimlanes), then
apply what holds up.
