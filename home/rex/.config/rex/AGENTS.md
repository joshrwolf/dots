# Rex configuration

Rex owns sessions, layout, focus, and program status; this Lua owns every Rex
semantic in dots. Rust reaches Rex only by running these actions
(`rex do <action> key=value`) or commands a view names; it never calls the Rex
API itself.

## Working with Rex

Rex is a fast-moving beta. Check behavior against the running version, not
memory or older notes.

- The Mac app's server is shared by every session on this machine. Never stop
  it or start a second server (`rex server run/stop`); either can end every
  session. `rex do` is a live connection, not an offline Lua runner. Scope it
  with native server/host options and isolate tests from real API calls.
- Reload (`rex config reload`) and install only when the user asks. Try live
  behavior in a throwaway session: `session.create` with `initial_windows`,
  then `rex kill <session>` to end it.
- Use the installed Rex skill/guide, `rex api describe <method>` and `rex keymap`
  for current APIs. CLI help and the connected host override older examples.
- `rex actions` lists server and config actions; `rex -C <id> actions` lists
  an app's own (`client.*`, `pane.*`, `session.*`, usable in `rex.bind` and
  `client.perform`), with the app's id from `rex client list`.

## App interactions

- Client actions control what the app displays; server focus alone does not.
  Use `dots.app.show` and confirm the intended client. Remote Control must be
  enabled for app-driving calls.
- Start programs through a terminal's `command`, not simulated typing. The
  login shell owns its environment; inspect process/input modes before sending
  input to an existing pane, and never send input to the agent's own pane.
- `dots.views` owns floating placement and focus/zoom restoration. Exercise
  those lifetimes in disposable sessions rather than relying on timing.
- Keep native key sequences and fuzzy matching. Use the connected client's
  action catalog and native terminal methods rather than copied API inventories.
- Status comes from programs through OSC7501; the shell also sends OSC133 prompt
  marks. Rex owns activity and notifications. Do not infer status from output or
  add a second notification system.

## Runtime

- Rex embeds gopher-lua: Lua 5.1 with no `bit`, `utf8`, `table.unpack`, or `//`.
  It accepts `goto` and `\x` escapes; do not use them. Leave byte and Unicode
  work to Rust.
- `package.path` includes `./?.lua`; keep modules under `dots.*`.
- `init.lua` runs at every load and reload and may only define bindings and
  actions; API calls fail there. Actions get 30 seconds and key functions 5.
- Module state persists across actions within one load and resets on reload;
  treat it as a cache.
- Actions may interleave at `rex.exec` and API calls. Do not hold a read across
  one and write it back.
- Rex answers a connection only for sessions it has attached, and each
  `rex do` is a new connection. Reach other sessions through `dots.sessions`.
- Actions inherit the server's environment, terminals the login shell's.
  `dots.process` resolves the selected machine environment with `mise env`
  from HOME, without loading a repository's development SDK. Mise must be on
  the server PATH or in its native `~/.local/bin` location; `MISE_BIN` can name
  the executable explicitly. Resolve programs with `dots.process.which`. A `rex do` script runs in the `rex`
  process with its caller's environment, so one typed into a terminal gets
  the login shell's. Go's start keys run worktrunk as a terminal's
  `command`, so worktrunk and its hooks get the login shell's environment too.
- `rex.on`, timers, and `rex.wait` work only in `rex do` scripts, so
  long-running Lua lives in `scripts/`.

## Structure

- `init.lua` lists features and nothing else. A feature is a `dots.*` module
  whose `install()` calls `rex.action` and `rex.bind` directly, so `rex keymap`
  names the real source line. Add a feature as one module plus one line in
  `init.lua`.
- Keep Rex-independent logic in plain functions over plain tables and test
  them; keep `rex.*` calls in thin adapters around them.
- Only `dots.views` decides how a view is placed and where focus goes when
  it closes. It also registers `views_dismiss`, which `dots-pick` runs on
  Esc.
- Dependencies point from features to shared modules (`dots.result`,
  `dots.process`, `dots.sessions`, `dots.snapshot`, `dots.start`,
  `dots.views`), never back.
  `dots.jumps` is shared too: it owns the jump list that navigating features
  record into, and also binds the toggle.
- Prefer Rex's built-in client actions over custom ones.
- A view is data for `dots-pick`, typed by `types/pick.lua`, which mirrors
  `src/pick/src/spec.rs`; change both together, and add every new view to
  `tests/contract.lua`.
- A live view names what invalidates it: Rex events through
  `dots.views.live` and `scripts/watch.lua`, or its own watch command, as the
  PR view uses `dots github watch follow`.
- Status views (Go's attention group, Attention) list any terminal whose
  program reports status, agents and long shell commands alike. `dots.agents`
  builds their rows and Go's tab rows alike: status, session, what, detail,
  one row per piece of work. The picker lines columns up by position across
  groups, so a view's other groups put matching values at the same position.
- `dots.snapshot` runs two bounded commands for places (`git worktree list`,
  `zoxide query`); larger domain data belongs in Rust behind a `dots` command.
- `dots.start` plans from local git and files alone. Only finding an existing
  PR session asks the network: `gh pr view` for the head branch, bounded to
  10 seconds; without an answer, the start creates a session.

## Style

- Modules are `local M = {}` … `return M`, with requires at the top and no
  globals. Use plain tables, not metatables or classes, unless a value needs
  behavior.
- Expected failures return `nil, err`, matching Rex's API. Actions raise with
  `dots.result.must`, which uses `error(msg, 0)` so the caller sees the message.
- Annotate public functions and data shapes with LuaCATS. `types/rex.lua` types
  the `rex` global; extend it when code reads a new field. Parenthesize
  multiple returns in function types, `fun(): (T?, string?)`, or the checker
  misreads them.
- Names: modules and actions in `snake_case`; action names have no dots.

## Checks and tests

`mise run //home:check` runs StyLua, selene, lua-language-server,
`rex config check`, and `tests/run.lua` in Rex's interpreter.
`cargo nextest run -p dots-pick --test contract` checks every view in
`tests/contract.lua` against the picker's format. Native machine groups deploy
tracked files; apply the selected machine setup to introduce a new path.

- The runner finds every `tests/<name>_test.lua`. A suite returns a table of
  `function(t)` keyed by name; assert with `tests/expect.lua`.
- `tests/isolate.lua` makes every `rex.call`, `rex.session.*`,
  `rex.block.call`, and `rex.invoke` fail unless a test replaces it, because `rex do` reaches the
  live server. It is not a sandbox: `rex.exec` still runs real commands, so
  stub `dots.process` where code shells out.
- Replace anything a test needs with `t.stub(tbl, key, value)`; the runner
  restores every stub after the test, pass or fail. Never assign over `rex` or a
  module field directly.
- Test code that reads or changes Rex against `fake_rex.install(t, fake)`. It
  logs each call in `fake.calls` (`fake.only(method, ...)` filters it) and fails
  on any call it does not know; extend it when code calls something new.
  `fake_rex.registered(t, feature)` returns a feature's actions, bindings, and
  modes.
- Run a subset from this directory by substring of `suite › name`:
  `rex --autostart=false do tests/run.lua --args '{"only":"go › go_to"}'`.

## Related code

- `home/zsh/.config/zsh/rex.zsh`: prompt marks and command status in Rex terminals.
- `home/nvim/.config/nvim/lua/config/keymaps.lua`: Ctrl+H/J/K/L hands focus back
  to Rex at a split edge.
- `src/pi/extensions/rex`: Rex's session-start orientation for Pi.
- `src/pick`: the picker views run in; its `AGENTS.md` covers the Rust side.
- `docs/github-watch.md`: the PR watch list behind the PR view.
