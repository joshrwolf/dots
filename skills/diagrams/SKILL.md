---
name: diagrams
description: Draw real diagrams (architecture, flow, swimlane, sequence, tree) and build self-contained HTML explainer pages around them, such as learning sheets, system walkthroughs and design overviews. Use whenever a picture would explain structure, flow or ordering better than prose, or when asked for a diagram, visual, learning sheet or explainer. Not for a three-item list or a single sentence.
---

# Diagrams

Schematic diagrams (boxes and connectors) are declared as data and laid out by
`assets/diagram-kit.js`. You decide what the diagram says and where things sit
on a grid. The kit computes box sizes from the text, routes every connector
through the gaps between boxes, places labels clear of boxes and other labels,
writes a text equivalent for screen readers, and checks the result. You never
write coordinates. That arithmetic is where hand-drawn diagrams fail.

Never draw a diagram as text art (box-drawing characters or ASCII arrows).

When the geometry itself is the explanation (a physical mechanism, a memory
layout, a cross-section), a grid of boxes is the wrong form. Hand-write one
small static `<svg>` with a `viewBox`, `role="img"` and an `aria-label` that
states what it shows, keep all text inside the viewBox, and explain it in
prose. The checker does not verify hand-written SVG, so keep it simple.

## Deliver

- Put each page in its own directory: the one the user names, otherwise
  `~/Documents/diagrams/<slug>/`. Copy `assets/kit.css` and
  `assets/diagram-kit.js` beside the page and start from
  `assets/template.html`. The page then works from `file://` with no network.
- Run `node <skill-dir>/scripts/check.mjs <page.html>`. It exits non-zero on
  any error or warning. Fix them by changing the structure. When a warning
  cannot be fixed (some graphs must cross), accept that one code in the spec
  with `"allow"` and say why in the caption or prose.
- Do not open the page in a browser and do not take screenshots to review it.
  Report the path; the user opens it.
- The check estimates text width without the real font and cannot judge
  colour contrast, hand-written SVG or phone layouts. Keep diagrams under
  about 1200px wide (it warns past that), and say in the report what was not
  verified.
- Light theme only, unless the user asks otherwise.

## Work in this order

1. **Decide what the diagram must show, in one sentence.** It becomes the
   required `title`, which is also the diagram's accessible name, and usually
   the line above the diagram. If you need two sentences, you need two
   diagrams.
2. **Pick the form** (details in [spec](references/spec.md#choosing-a-layout)):

   | Content | Form |
   |---|---|
   | Components and who calls whom | graph, tiers as rows |
   | A process or pipeline | graph, one row or column per stage |
   | Work split across owners | graph, one row per owner (swimlane) |
   | Requests between a few actors over time | sequence |
   | A hierarchy or decision tree | graph with `"bus": true`, levels as rows |
   | A physical or spatial mechanism | hand-written SVG, see above |
   | Quantities | a chart, see [charts](references/charts.md) |

3. **Place the boxes first.** Write the node list with `col` and `row` before
   any edge. Put things that happen in order along one axis, and things of the
   same kind in the same row or column. Leave a cell empty rather than squeeze.
   Past 12 nodes, split into an overview plus detail diagrams.
4. **Add edges, then labels.** Label an edge only when its meaning is not
   obvious from its two ends, in one to three words.
5. **Run the check** and fix what it reports: reorder columns, move a box,
   shorten a label, split the diagram. Use `fromSide`/`toSide` only to fix a
   route whose problem you can name.
6. **Write the prose around it.** The diagram carries names and structure. The
   explanation, caveats and numbers go in the text, in click-through `detail`,
   or in `steps`.

## Rules that matter most

- Never invent facts, numbers or quotes to fill a diagram or a page. Ground
  claims in the code, docs or data you read, and mark what is unknown.
- Colour encodes a category, never a sequence: at most three tones plus
  neutral, the same tone for the same kind of thing, and a legend whenever
  tone carries meaning (the check enforces the legend).
- Box labels name the thing in sentence case. Subtitles are five words or
  fewer. Detail goes in `detail`, not in the box.
- The words carry the structure as well as the lines: number steps when order
  matters, and let the title say what the diagram shows.
- One diagram per idea. A page builds from an overview to detail views of the
  same system, reusing the same names and tones throughout.
- Prefer a stepper (`steps`) over a wall of text for a walkthrough: each step
  highlights the boxes and edges it talks about.
- When you extend an existing page, keep its names, tones and layout; change
  only what the request covers.

## References

- [spec](references/spec.md): the JSON spec for graph and sequence diagrams,
  layout recipes, interaction (`detail`, `steps`, `legend`), check codes.
- [design](references/design.md): visual and writing rules for diagrams and
  for explainer pages.
- [charts](references/charts.md): choosing and styling charts when the content
  is quantities.
- [sources](references/sources.md): where this guidance comes from and how to
  sync it with upstream.
