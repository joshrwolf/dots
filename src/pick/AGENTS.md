# Picker

The library is a generic terminal picker that knows nothing about Rex
or any data source. `spec` describes a picker as strict JSON for hosts in
other languages. The `dots-pick` binary hosts a spec for Rex: a Lua action
returns the view, and the chosen row id goes to the view's accept or shortcut
action through `rex do`. A shortcut with `query` takes the typed line as input:
it fires with or without a row and its action receives `{ id?, query }`; on a
blank line it asks for its `hint` instead of running. With `--dismiss`,
closing the picker without choosing runs that action before it exits. Enter and other
shortcuts send only `{ id }`, because the schema Rex builds from an action's
`args` forbids undeclared fields. The line filters rows the same way either
way. It never calls the Rex API itself.

- Actions run in place while the picker stays drawn. Typed actions return
  close, push, replace or back; the page stack preserves query, scope and
  selection and stops hidden-page feeds/previews. Rex actions close on success.
  Failures stay under the prompt rule without losing the current page. Lua
  owns its argument validation and errors; only failures before drawing fall
  back to plain text. Preview scrolling uses Shift+arrows/page keys or the
  wheel over its pane; list clicks select rows without executing actions.

- Keep the model a pure function of items, query, cursor, and width, and test
  it as tables rather than through a terminal.
- Measure widths in terminal display cells and cut, edit, and move the caret
  by grapheme cluster, the unit nucleo match positions index. Never use byte or
  `char` counts for layout; test wide glyphs, combining marks, and emoji
  sequences when changing `columns`, the model, or drawing.
- Rows, labels, the prompt, and the query are each one line: host text goes
  through `text::display`, and typed, pasted, and initial queries through
  `text::input`.
- Colours come only from `Palette`; a theme adapter is `pick.toml` in the
  shared theme package. Specs name roles, never colours.
- `spec` accepts `{}` wherever it accepts a list, because Lua JSON encoders
  cannot tell an empty array from an empty object. `home/rex/.config/rex/types/pick.lua`
  mirrors the format; change both together. `tests/contract.rs` runs the
  Rex views' Lua builders in Rex's interpreter, isolated from any running
  server by `tests/isolate.lua`, and checks every view they produce.
- All asynchronous work (the live feed, previews, and actions) runs on the one
  `Background` runtime, with asynchronous I/O and commands started through
  `dots-process`, so dropping a job kills its command's process group. A job
  that panics ends the picker: Ratatui's hook restores the terminal, and the
  loop resumes the panic after cleanup. Do not add another terminal guard or
  catch panics; `tests/acceptance.rs` checks this on a pseudo-terminal with
  a `testing` fixture. Restore the terminal before shutting the runtime
  down, including after a failed initialization.
- Previews debounce selection changes and cache recent command outcomes;
  cached failures retry, and an interval refreshes cached successes. Pass the
  preview pane's cell dimensions as COLUMNS/LINES, from the one body split
  (`split` in `run.rs`) that also draws the pane beside or below the list;
  replacing live rows or resizing invalidates cached previews. Previews render
  SGR colors and attributes as pane-local spans; discard all other terminal
  escapes.
- A live view refreshes on every line of its watch command, the first one
  included, and runs at most one fetch at a time. A failed or rejected refresh
  keeps the rows shown and says so; it never empties the list.
- Never start, stop, or act on a real Rex server from tests; see
  [working with Rex](../../home/rex/.config/rex/AGENTS.md#working-with-rex).
