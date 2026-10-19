# Pi presentation library

`@joshrwolf/pi-dots/ui` supplies reusable presentation components without
registering extensions, selecting themes, or starting resources on import.

## Use

Compose rows, notices, output, and source/diff previews through the exported
TypeScript interfaces. Typed consumers in
[ui-foundation.test.ts](../tests/ui-foundation.test.ts) and
[ui-export.test.ts](../tests/ui-export.test.ts) show custom-tool and export use.

A presentation projects recorded observations into a readonly model, then
updates a native Pi component. Renderer ownership and shell mode stay fixed
for the whole row; unsupported records need a conservative visible fallback.

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
- Native HTML exports and TUI rendering have different paths; test both. Pi does
  not record nested codemode results, so child source/diff views are unavailable.

See [AGENTS.md](../AGENTS.md) for contributor rules. Public types and tests own
the component API reference.
