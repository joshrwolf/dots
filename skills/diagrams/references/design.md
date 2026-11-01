# Design rules

`kit.css` already sets type, colour and spacing. These rules are about the
choices it cannot make for you.

## Diagrams

- **Complexity budget.** At most 12 nodes, three tones plus neutral, five-word
  subtitles, three-word edge labels. When a diagram strains these, it is two
  diagrams: an overview whose boxes link to detail views, or a main path plus
  a separate diagram for the exception path.
- **Colour encodes category.** Same kind of thing, same tone. Do not cycle
  tones along a sequence. Neutral is right for most boxes; use `accent` for the
  one thing the reader should look at.
- **Name things the way the reader does.** Use the names that appear in the
  code, the UI or the docs, in sentence case, so the reader can search for
  them. Do not invent abbreviations.
- **Text belongs outside the picture.** No paragraphs, headings or numbers in
  boxes. The diagram shows structure; prose, `detail` and `steps` explain it.
- **Dashed means something.** Use dashed edges for one meaning only (optional,
  asynchronous, proposed) and say which in the legend or caption.
- **Hypotheses look different from facts.** When a diagram mixes what exists
  with what is proposed, make the proposed part dashed or a separate diagram,
  and say so in the caption.
- **Width is a reading budget.** A diagram scales down to fit its column and
  then scrolls sideways rather than shrinking text further. Past about 1200px
  it will scroll on most screens: restack it, split it, or shorten labels.

## Explainer pages

- Open with what the reader will understand by the end, then the overview
  diagram, then sections that zoom in. Each section starts with its claim.
- Every diagram has a sentence before it saying what to look at, and the text
  after it adds what the picture cannot: reasons, caveats, numbers, sources.
- Ground every claim. Cite code as `<span class="src">path:line</span>`
  chips (click copies the path), mark what is proven, measured or a
  hypothesis, and date what will go stale. When a fact is missing, say so
  rather than estimate it; never invent statistics, quotes or examples
  presented as real.
- Use tables for reference material (settings, metrics) and `details` for
  material most readers skip. Use `.note.key`, `.note.warn` and `.note.risk`
  sparingly for the few things that change decisions.
- A page is a living document. When the reader asks about part of it, amend
  that part (zoom in with a new diagram or section, correct a claim, add a
  glossary entry) instead of appending questions and answers. Keep the
  page's existing names, tones and structure.
- Interactive pieces earn their place: steppers for walkthroughs, click
  details for dense nodes, a self-check quiz for learning pages. Controls are
  real buttons and inputs with labels. Do not add motion or hover effects for
  decoration.
- Avoid template tells: an all-caps label above every heading, a gradient
  hero, a big number with a small label, identical rounded cards for
  everything, emoji as icons.
