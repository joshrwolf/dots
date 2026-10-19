# Pi presentation library

Import `@joshrwolf/pi-dots/ui` from companion-package code. Importing it does
not register extensions, select a theme, or start resources.

- `PriorityRow` lays out semantic segments using Pi's `HStack`. Set
  `layout: "inline"` to keep start/end groups adjacent, or `layout: "edges"`
  (the row default) to anchor them to opposite edges. Both use the same
  shrink policy. `groupSeparator` chooses the boundary between nonempty groups
  (one space by default); it disappears if either group is omitted. A segment’s
  `separatorBefore` overrides its intra-group boundary only when a preceding
  segment survives, allowing related metadata to use spaces rather than dots. Optional
  segment `padTo` supplies soft right-padding, surrendered before any content
  shrinks. It neither truncates longer text nor changes `minWidth`.
  Segment `width` instead clips and pads to a preferred cell size, overriding
  `padTo`; that whole cell yields under its priority and `minWidth`, rather than
  losing padding first. Use it for aligned list columns. Segments support `bold`
  emphasis and optional `badge: "error" | "warning"` styling: padded text with a
  theme background and contrast-adjusted foreground, never color-only meaning. Row `indent` reserves
  leading layout cells without preserving untrusted whitespace; it reduces the
  content budget and is clamped to leave one content cell in tiny panes. Priorities decide
  what survives narrow widths; `clip` and `omit` control shrinking. Middle
  clipping preserves path tails, desired minimum widths protect subjects, and
  layout-owned separators disappear with their optional segments. An omitted
  `minWidth` means zero: pass one shrinks in ascending priority to explicit
  floors; pass two lifts those floors in the same order. Critical status text
  needs its own floor if it must survive another segment's reservation.
- `Notices`, `OutputView`, and `ToolCard` compose bounded recorded output and
  visible recovery information. `ToolCard.update(model, theme)` accepts separate
  heading/status segments, notices, a body, and its caption. Its `layout` defaults
  to `"inline"`; callers can explicitly choose `"edges"`. Header layout does not
  cap subjects or constrain the expanded body. Alternatively, pass `header: RowModel`
  instead of heading/status to choose arbitrary order, grouping, separators and
  soft slots; an unspecified header layout still defaults to inline.
  `captionPolicy: "when-not-in-header"` suppresses a caption only while its full
  text is visible in the rendered header; the default is `"always"`. Empty captions
  consume no row. `DetailView` also accepts a width-aware caption callback and gives
  expanded content a shared margin without owning its layout. `ToolCard` uses
  Pi's `VStack`.
- `SourceView` and `DiffView` provide numbered source and adaptive recorded-patch
  previews. Split diffs default to content-sized columns within the available
  width; `DiffViewOptions.widthPolicy: "fill"` opts into equal full-width columns.
  Narrow or wrap-heavy diffs still use the unified view. Use `parsePatch` for untrusted unified patches. The painter uses
  `readableForeground` to retain syntax hues where possible while targeting
  4.5:1 contrast on diff backgrounds. Its bounded cache keys concrete color
  pairs, not theme identities; terminal palette quantization can differ.
- `createToolRenderers` converts a typed presentation descriptor into Pi's native
  `ToolRenderers`. It can be spread into a custom tool definition directly.
- `selectToolRenderers` is an optional, thin resolver for selected tool names.
  Selection fixes ownership for the entire row; unsupported records should be
  displayed conservatively by that presentation, not delegated mid-row.

Tool presenters compose action → subject → metadata with two-space group
boundaries and spaces within related metadata (such as `+3 −2`). Subjects start
at a stable column; metadata follows nearby rather than at the screen edge. Timings under
one second appear only when expanded. Redundant full-path captions are suppressed
while the header contains the full path. Their five-column soft action slot steadies short labels without
aligning whole rows or maintaining transcript-wide column widths. Optional timing
is omitted before the subject is clipped; expanded content retains full subjects.

`StateGutter` decorates any component with a static two-cell state column.
`ToolCardModel.state` opts into it for the header only; undecorated callers remain
unchanged. Callers supply `ActivityState` from recorded/execution facts:
`receiving` (·), `queued` (◦), `running` (◌), `done` (blank), `failed` (✗),
`cancelled` (−), or `unknown` (?). No row timers or time-derived animation are used:
Pi cannot expose viewport visibility and off-screen animation can force redraws.
Tool identities are bold; success words/checks are omitted. Tool presenters use
the failure gutter plus a diagnostic, not a redundant failed badge; badges remain
available to other callers. `ToolCardModel.noticeIndent` softly indents diagnostics
(default zero), yielding space to retain up to 16 content cells in narrow panes.
`Notices.update` accepts the same indentation as its optional third argument.

`SoftEditor` extends Pi's `CustomEditor`, softening only its current border
callback during rendering. It retains native editing and working-status APIs;
use the host's editor factory rather than constructing an independent editor.

A presentation supplies `shell`, a pure `project(observation)` function, and a
`create(invalidate)` factory returning a native `Component` with
`update(model, theme, expanded)`. The view represents the complete row. Its
result replaces the call visually in the TUI; non-template HTML exports retain
both serialized slots. `self` shells receive adapter-managed `outputPad`;
`default` shells keep Pi's framing.

```ts
import { createSyntaxService, SourceView } from "@joshrwolf/pi-dots/ui";

const syntax = createSyntaxService(); // Inert until requested; usable before session_start.
const preview = new SourceView({
  source: "const answer = 42;",
  path: "example.ts",
  startLine: 20,
  theme,                            // Supplied by Pi.
  syntax,
  invalidate,                       // Supplied by the owning surface/renderer.
});
```

The owner shares one syntax service across its previews and calls
`syntax.dispose()` at runtime shutdown. Disposal is idempotent and handles
pending initialization. Preview callbacks ignore late results after disposal;
`syntax.failure` exposes a failed engine/grammar request while source remains
readable. Do not start per-row timers: Pi provides no tool-row disposal hook.

Use `setTheme` when supplying a new theme and `invalidate` when the host
invalidates a component. Do not cache styled output outside that lifecycle.
Record text stays separate from display sanitization. Library budgets limit
preparation and output; omission markers distinguish display limits from tool
truncation.

The codemode consumer in `features/tools/codemode.ts` composes the same rows,
notices, output and source views from recorded call summaries. It does not invent
child source/diff views: Pi records no nested results. Its native VM contract and
live/replay tests are in `tests/codemode-contract.test.mjs` and
`tests/ui-codemode.test.ts`.

Typed custom-tool and export consumers are in `tests/ui-foundation.test.ts` and
`tests/ui-export.test.ts`; layout, editor, and configuration contracts are in
`tests/ui-presentation.test.ts`. Development boundaries and verification standards
are in [AGENTS.md](../AGENTS.md).
