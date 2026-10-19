# Pi presentation library

`@joshrwolf/pi-dots/ui` supplies reusable presentation components without
registering extensions, selecting themes, or starting resources on import.

## Use

Compose `ActivityCard` with one explicit header model, notices, and optional
recorded details. `activityHeader` shares identity/subject/metadata layout across
tools and background work: outcomes sit beside subjects, while explicitly
secondary metadata uses the trailing edge. Native Pi components remain directly usable. Typed consumers in
[ui-foundation.test.ts](../tests/ui-foundation.test.ts) and
[ui-export.test.ts](../tests/ui-export.test.ts) show custom-tool and export use.

Use `Panel` for read-only dashboards and inspection sections: a light frame,
a clear heading, aligned content, and a quiet footer. Keep transcript activity
unboxed. Panels own no input; an enclosing `Inspector` owns scrolling.

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
- Tool rows have no disposal or viewport-visibility contract. Do not start per-row
  timers or watchers; activity markers are static facts, not animations.
- Apply theme changes through the component lifecycle. Keep original records apart
  from sanitized display text, and do not retain stale styled output.
- Bound preparation and rendering. Omission markers distinguish display limits
  from truncated tool results.
- Inspectors use explicit viewport bounds with the public `ScrollView` API.
  Pi 1.3 editor slots and overlays render through legacy containers; a nested
  `ScrollView` alone does not receive layout bounds. Verify scrolling in the host,
  not just component snapshots. Inspection bodies are read-only.
- Native HTML exports and TUI rendering have different paths; test both. Pi does
  not record nested codemode results, so child source/diff views are unavailable.

See [AGENTS.md](../AGENTS.md) for contributor rules. Public types and tests own
the component API reference.
