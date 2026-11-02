# Pi presentation library

`@joshrwolf/pi-dots/ui` supplies reusable presentation components without
registering extensions, selecting themes, or starting resources on import.
`Panel`, `PriorityRow`, and the text helpers live in `@joshrwolf/pi-kit/ui`,
and are re-exported here.

## Use

Compose `ActivityCard` with one explicit header model, notices, and optional
recorded details. `activityHeader` shares identity/subject/metadata layout across
tools and background work: outcomes sit beside subjects, while explicitly
secondary metadata uses the trailing edge. Native Pi components remain directly usable. Typed consumers in
[ui-foundation.test.ts](../tests/ui-foundation.test.ts) and
[ui-export.test.ts](../tests/ui-export.test.ts) show custom-tool and export use.

Use one `Panel` per inspection surface: its heading, body, and action footer
form a light, padded frame. `Inspector` already supplies that frame and owns the
native `ScrollView`; pass unframed body content to `setContent`, not another
`Panel` or `ScrollView`. For a native picker, put its `SelectList` directly in a
`Panel` body and route input to the list. `Inspector` pins an optional one-row
input accessory below its scrollable body, inside the same frame. `Panel`
forwards body mouse events but owns no selection or scrolling. Reserve room for the heading/footer
when budgeting body rows (`inspectorHeight` does this for overlays); keep the
picker's visible-row budget responsive to terminal height. Feature owners supply
facts, actions, and status hints, and the host retains overlay focus and the
editor draft. Persistent activity and transcript rows stay unboxed.
Companion activity sections share headings and default row allowances; feature
owners select relevant records and report omissions.

A presentation projects recorded observations into a readonly model, then
updates a native Pi component. Feature owners supply facts and actions; host
adapters mount components and own subscriptions. Renderer ownership and shell mode stay fixed
for the whole row; unsupported records need a conservative visible fallback.

Preview the shipping components with `mise run //src/pi:workbench` in an
isolated, provider-free host. See the [workbench guide](../dev/README.md).

Routine success stays quiet. Accent identifies work; warnings and errors retain
textual markers so color is never the only signal. Diagnostics remain visible
when details are collapsed, and metadata yields before important outcomes.

## Constraints

- Pi owns rendering, layout primitives, input, themes, and scheduling. Presentation
  must not change tool execution or reconstruct missing history from current files.
- Share one lazy syntax service per runtime and dispose it at shutdown. Ignore late
  initialization results after disposal; source stays readable if highlighting fails.
  The service records initialization, grammar, and cleanup errors in `failure`.
  Explicit preview `prepare()` calls return failures to their caller; `render()`
  owns detached preparation and redraw errors, exposing them in the view’s `failure`.
- Tool rows have no disposal or viewport-visibility contract. Do not start per-row
  timers or watchers; activity markers are static facts, not animations.
- Apply theme changes through the component lifecycle. Keep original records apart
  from sanitized display text, and do not retain stale styled output.
- Bound preparation and rendering. Omission markers distinguish display limits
  from truncated tool results. Pi can render a row on every frame: reuse styled
  rows until their content, width or theme changes, and show source that is
  still streaming as plain text (`highlight: false`) until its arguments complete.
- Inspectors set explicit viewport bounds with the public `ScrollView` API.
  Overlays render the custom component at a width only and pass no viewport
  height into a nested `ScrollView`; keep the explicit bound. Verify
  scrolling in the host, not just component snapshots. Inspection bodies are read-only.
- Native HTML exports and TUI rendering have different paths; test both. Pi does
  not record nested codemode results, so child source/diff views are unavailable.

See [AGENTS.md](../AGENTS.md) for contributor rules. Public types and tests own
the component API reference.
