# Diagram spec

A diagram is a JSON object in a script tag. The kit renders it as a
`<figure>` right after the tag, or inside the element named by
`data-target="<id>"`.

```html
<script type="application/json" data-diagram>
{ "id": "overview", "title": "…", "nodes": [ … ], "edges": [ … ] }
</script>
```

The JSON must be strict: double quotes, no comments, no trailing commas. The
check reports a parse error with its position.

## Graph (`"type": "graph"`, the default)

| Field | Meaning |
|---|---|
| `id` | Names the diagram in check output. |
| `title` | Required. The one-sentence claim; also the diagram's accessible name. |
| `caption` | Optional line under the figure. |
| `nodes[]` | `id`, `label`, `col`, `row` (non-negative integers, one node per cell); optional `sub`, `tone`, `group`, `detail`. |
| `edges[]` | `from`, `to`; optional `label`, `style: "dashed"`, `fromSide`/`toSide` (`top`, `right`, `bottom`, `left`), `id`. The default id is `from>to`; a second edge between the same pair becomes `from>to#2`. An edge from a node to itself draws a loop over its top right. |
| `groups[]` | `id`, `label`, `tone`. Members name it with `group`. A group's members must fill a rectangle of cells with no other node inside it, and groups cannot nest or overlap. |
| `bus` | `true` lets edges from the same source share their trunk, as in a tree or org chart. Otherwise parallel edges run side by side. |
| `legend[]` | `{ "label", "tone" }` entries shown under the figure. Required whenever two or more tones are used. |
| `steps[]` | Walkthrough: `{ "title", "body", "nodes": [ids], "edges": [edge ids] }`. An edge brings its two ends with it. Unknown ids are errors. |
| `detailHint` | Text in the detail panel before anything is selected. |
| `allow[]` | Warning codes accepted on purpose; see [check codes](#check-codes). |

`tone` is one of `neutral`, `accent`, `teal`, `amber`, `purple`, `coral`,
`blue`, `green`, `red`. Keep `blue`, `green`, `amber` and `red` for nodes that
really mean information, success, warning or failure; use `teal`, `purple`
and `coral` for plain categories, and `accent` for the one thing to look at.

`detail` and step `body` take either an HTML string or `"#id"`, the id of a
`<template>` (or any element) holding the content. Use templates for anything
longer than a sentence; HTML inside JSON strings is hard to read and escape
(single quotes for HTML attributes avoid escaping).

Column widths fit the widest label in the column and row heights the tallest
box in the row, so boxes in a column share a width and boxes in a row share a
height. Labels wrap at about 200px. A column or row with no nodes still takes
space, which is a way to open a gap.

Edge labels sit on their line over a background patch when there is room,
otherwise just beside it, and never on a box, a group heading, another label
or another edge. Gaps widen to make room.

## Sequence (`"type": "sequence"`)

| Field | Meaning |
|---|---|
| `title` | Required, as for graphs. |
| `participants[]` | `id`, `label`; optional `sub`, `tone`, `detail`. Left to right in this order. |
| `messages[]` | `{ "from", "to", "label", "style" }`, with `style: "reply"` (dashed) for responses, or a note `{ "note": "text", "over": [id] or [id, id] }`. A message to the same participant draws a self-call. |
| `autonumber` | `true` numbers the messages. |
| `steps[]` | As for graphs, with `messages` holding indexes into `messages[]` (notes count). |
| `legend[]`, `caption`, `allow[]` | As for graphs. |

Keep to seven participants and twenty messages; split longer exchanges at a
natural boundary, such as the cache-hit path versus the miss path.

## Accessibility

Each figure carries a visually hidden text version: for a graph, every box
with what it leads to; for a sequence, the ordered messages. Boxes with
`detail` are focusable buttons (Enter or Space opens the detail). Because the
text version is generated from labels, the labels must make sense read
aloud: write "Index check" rather than "IC", and put the meaning of a dashed
or coloured edge into its label or the caption.

## Choosing a layout

Pick by the shape of the content, then place on the grid.

- **Chain or pipeline.** One row, in order. Past five or six steps, snake:
  run the second row right to left under the first, joined by one edge down at
  the turn, and number the steps in their labels so the order survives. Merge
  trivial steps ("Validate and register") rather than shrinking text. A long
  chain can also run down a column.
- **Tiers (architecture).** Rows are tiers: callers on top, services in the
  middle, storage and external systems at the bottom. Put the boxes a request
  passes through in one column so the main path is straight, and side systems
  in neighbouring columns. Group each service's parts.
- **Swimlane.** One row per owner, each row a group. A hand-off to another
  lane stays in the same column, directly below or above; move one column
  right only when the next step is in the same lane.
- **Tree, org chart, decision tree.** Levels are rows. Count the leaves first:
  they set the columns. Put each parent in the column centred over its
  children, which works best with odd child counts; leave the column between
  two children empty when a parent has two. Set `"bus": true`. For a decision,
  put the answer in the child's label ("No: escalate") or as a one-word edge
  label.
- **Loops and retries.** Draw the back edge; the router takes it around the
  outside of the boxes. A self-loop is an edge to the same node.
- **Timeline.** Milestones in order along one row, dates in the subtitles.
  The grid spaces them evenly, so when uneven spacing matters, use a table or
  prose instead.
- **Containment.** Groups are flat rectangles. For nesting (a region holding
  clusters holding services), draw the outer level, then a separate diagram
  zoomed into one part.
- **Before and after.** Two diagrams with the same grid and names, side by
  side in a `.cols` block, beats one diagram with dashed "new" edges.

When the check reports crossings, try swapping two columns or moving the
busiest node to the middle column before reaching for `fromSide`/`toSide`.

## Check codes

Errors always fail. Warnings fail too unless the spec lists their code in
`allow`:

| Code | Raised when |
|---|---|
| `nodes` | More than 12 nodes, 7 participants or 20 messages |
| `tones` | More than three tones besides neutral |
| `legend` | Two or more tones without a legend entry for each |
| `words` | Long labels, subtitles over five words, edge labels over three |
| `crossings` | Any two edges cross |
| `width` | The diagram is wider than 1200px |
| `group` | Edges cross a group's heading on every stretch |
| `label` | An edge label had to cover part of another edge |

## What the kit guarantees, and what it does not

By construction: no connector crosses a box, every connector starts and ends
at a box edge, parallel connectors run apart, sequence notes stay inside the
figure, and every box is as wide as its measured text. The check fails when
labels overlap boxes or each other.

Measured, not proven: text widths in the checker are estimates (wide scripts
count a full em per character); the browser measures with the real font and
lays out again, so a page is right in the browser even when the estimate is
a little off.

Not handled, so design around it: nodes spanning several cells, nested
groups, curved connectors, diagonal lines and free positioning. Each one
would bring back hand-placed geometry.
