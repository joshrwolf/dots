-- Go: one picker over everything worth jumping to, and where work starts.
-- Agents that want the user come first, then every other tab in every
-- session, showing the status of the agent at work in it; worktrees (@) and
-- directories (/) wait behind their scopes so they never crowd live state.
-- Enter goes to the selected row; the start keys below take the typed line as
-- a PR URL or a Linear issue, or start a new session in the selected row's
-- repo (see `dots.start`).
local agents = require("dots.agents")
local app = require("dots.app")
local hosts = require("dots.hosts")
local jumps = require("dots.jumps")
local must = require("dots.result").must
local sessions = require("dots.sessions")
local snapshot = require("dots.snapshot")
local start = require("dots.start")
local status = require("dots.status")
local views = require("dots.views")

-- Start keys, pressed with Ctrl. The picker keeps a, b, c, e, f, g, h, j, k,
-- n, p, q, u, and w for editing and movement, Rex takes h, j, k, and l for
-- panes and s for splits, and i and m arrive as Tab and Enter; d, o, r, t, v,
-- x, y, and z are free.
local KEYS = { pr = "r", issue = "t", new = "o", host = "y" }

local SHELL = "com.superlogical.terminal.shell"

---@class dots.Go
local M = {}

local basename, tilde = snapshot.basename, snapshot.tilde

---Row ids carry what `go_to` needs: `tab|<session>|<window>`, `block|<session>|<block>`, `dir|<path>`.
---@param id string
---@return { kind: "tab"|"block"|"dir", session?: string, window?: string, block?: string, path?: string }?
function M.parse_id(id)
  local kind, rest = id:match("^(%a+)|(.+)$")
  if kind == "tab" then
    local session, window = rest:match("^([^|]+)|([^|]+)$")
    if session then
      return { kind = kind, session = session, window = window }
    end
  elseif kind == "block" then
    local session, block = rest:match("^([^|]+)|([^|]+)$")
    if session then
      return { kind = kind, session = session, block = block }
    end
  elseif kind == "dir" then
    return { kind = kind, path = rest }
  end
  return nil
end

---The directory a row stands for: a place's path, or the directory of the
---terminal that describes a tab or of an agent's.
---@param snap dots.Snapshot
---@param id string
---@return string?
function M.place(snap, id)
  local target = M.parse_id(id)
  if not target then
    return nil
  elseif target.kind == "dir" then
    return target.path
  end
  for _, session in ipairs(snap.sessions) do
    if session.id == target.session then
      if target.kind == "tab" then
        for _, tab in ipairs(M.tabs(session)) do
          if tab.window_id == target.window then
            return tab.block.cwd
          end
        end
      end
      for _, block in ipairs(session.blocks) do
        if block.id == target.block then
          return block.cwd
        end
      end
    end
  end
  return nil
end

---@class dots.Tab
---@field window_id string
---@field block dots.Block The terminal that describes it: its most urgent agent that does not need attention, or its focused terminal.
---@field attended boolean Every agent in it needs attention, so their rows stand for it.

---A session's tabs in order.
---@param session dots.Session
---@return dots.Tab[]
function M.tabs(session)
  local tabs, index = {}, {} ---@type dots.Tab[], table<string, integer>
  local busy, waiting = {}, {} ---@type table<string, dots.Block>, table<string, boolean>
  for _, block in ipairs(session.blocks) do
    local at = index[block.window_id]
    if not at then
      tabs[#tabs + 1] = { window_id = block.window_id, block = block, attended = false }
      index[block.window_id] = #tabs
    elseif block.focused then
      tabs[at].block = block
    end
    local record = block.status
    if record and status.needs_attention(record.state) then
      waiting[block.window_id] = true
    elseif record then
      local best = busy[block.window_id]
      if not best or status.rank(record.state) < status.rank(best.status.state) then
        busy[block.window_id] = block
      end
    end
  end
  for _, tab in ipairs(tabs) do
    tab.block = busy[tab.window_id] or tab.block
    tab.attended = waiting[tab.window_id] == true and busy[tab.window_id] == nil
  end
  return tabs
end

---The view for a snapshot. Pure: Rex and command access is in `dots.snapshot`,
---and the jumps that order tabs come from `dots.jumps`.
---@param snap dots.Snapshot
---@param recent? dots.Location[] Tabs most recently jumped to first; they lead the tabs.
---@param host? string The owning host's display name.
---@return pick.View view
function M.view(snap, recent, host)
  local items = {} ---@type dots.AgentRow[]

  for _, entry in ipairs(agents.wanting(snap)) do
    items[#items + 1] = { group = "attention", entry = entry }
  end
  local tabs = {} ---@type { location: dots.Location, item: dots.AgentRow }[]
  for _, session in ipairs(snap.sessions) do
    for _, tab in ipairs(M.tabs(session)) do
      if not tab.attended then
        tabs[#tabs + 1] = {
          location = { session_id = session.id, window_id = tab.window_id },
          item = { group = "tabs", entry = { session = session, block = tab.block, tab = true } },
        }
      end
    end
  end
  local function location_key(tab)
    return jumps.key(tab.location)
  end
  for _, tab in ipairs(jumps.order(tabs, location_key, recent or {})) do
    items[#items + 1] = tab.item
  end
  local rows = agents.rows(items, snap)

  -- Places are listed even when a tab is open there: they are where new
  -- work starts (Ctrl+O), and Enter on one goes to its session if it has one.
  local listed = {} ---@type table<string, boolean>
  local function directory(group, path)
    if listed[path] then
      return
    end
    listed[path] = true
    rows[#rows + 1] = {
      id = "dir|" .. path,
      group = group,
      cells = {
        { text = "", kind = "tag" },
        { text = snapshot.name(path, snap) },
        { text = tilde(path, snap.home), kind = "path", role = "muted" },
      },
      search = path,
    }
  end
  -- A repo's primary checkout is a directory, not one of its worktrees.
  local repos = {} ---@type table<string, boolean>
  for _, path in ipairs(snap.dirs) do
    repos[path] = true
  end
  for _, path in ipairs(snap.worktrees) do
    if not repos[path] then
      directory("worktrees", path)
    end
  end
  for _, path in ipairs(snap.dirs) do
    directory("dirs", path)
  end

  -- Lined up with the agent columns: no status, the name where the session goes.
  local place = { { width = { cells = 1 } }, { width = { content = 32 } }, { width = "fill" } } ---@type pick.Column[]
  return {
    prompt = host and ("go [" .. host .. "]› ") or "go› ",
    accept = "go_to",
    shortcuts = {
      { key = KEYS.pr, label = "review PR", action = "go_start_pr", query = true, hint = "URL" },
      {
        key = KEYS.issue,
        label = "start issue",
        action = "go_start_issue",
        query = true,
        hint = "ID or URL",
      },
      { key = KEYS.new, label = "new session", action = "go_start_new" },
      { key = KEYS.host, label = "switch host", action = "go_switch_host" },
    },
    groups = {
      { id = "attention", label = "needs attention", columns = agents.columns },
      { id = "tabs", label = "tabs", columns = agents.columns },
      { id = "worktrees", label = "worktrees", columns = place, scope = "@" },
      { id = "dirs", label = "directories", columns = place, scope = "/" },
    },
    rows = rows,
  }
end

---Show a row's session, focusing its terminal or creating a session for a
---place, and record the jump.
---@param ctx rex.Context
---@param args { id: string }
function M.go_to(ctx, args)
  local target = M.parse_id(args.id)
  if not target then
    error("go_to cannot read row id " .. args.id, 0)
  end
  local from = jumps.here(ctx)
  if target.kind == "tab" then
    app.show(target.session, target.window, ctx.session_id, ctx.client_id)
    jumps.record(from, { session_id = target.session, window_id = target.window })
  elseif target.kind == "block" then
    must("focusing the terminal", sessions.focus(target.session, target.block))
    -- Name the window, so the app shows the one the server just made active.
    local window = sessions.window_of(target.session, target.block)
    app.show(target.session, window, ctx.session_id, ctx.client_id)
    jumps.record(from, { session_id = target.session, window_id = window })
  else
    local path = target.path --[[@as string]]
    -- The picker that chose the row is still open; its directory is not a place.
    local session = snapshot.session_at(snapshot.collect({ exclude = ctx.block_id }), path)
    if not session then
      local created = must(
        "creating a session for " .. path,
        rex.call("session.create", {
          label = basename(path),
          initial_windows = {
            { layout = rex.layout.block({ flavor = SHELL, options = { cwd = path } }) },
          },
        })
      )
      session = created.session_id
    end
    app.show(session, nil, ctx.session_id, ctx.client_id)
    jumps.record(from, { session_id = session })
  end
end

---Start work from the typed line: plan it, or fail with why the line is
---invalid, then go to the session at its worktree or create one.
---@param ctx rex.Context The picker's, which runs in the directory Go was opened from.
---@param kind dots.StartKind
---@param args { id?: string, query: string }
function M.start(ctx, kind, args)
  local snap = snapshot.collect({ exclude = ctx.block_id })
  local here = ctx.session_id and ctx.block_id and sessions.cwd(ctx.session_id, ctx.block_id)
  local plan, err = start.plan(kind, args.query, here or nil, snap.home)
  if not plan then
    error(err, 0)
  end
  start.open(ctx, plan, snap)
end

---Start a new session in the selected row's repo, or fail with why there is
---none, then go to it.
---@param ctx rex.Context The picker's.
---@param args { id?: string }
---@param random? fun(n: integer): integer Defaults to `math.random`.
function M.start_new(ctx, args, random)
  local snap = snapshot.collect({ exclude = ctx.block_id })
  local plan, err = start.plan_new(args.id and M.place(snap, args.id), random or math.random)
  if not plan then
    error(err, 0)
  end
  start.open(ctx, plan, snap)
end

-- Session RPC routing alone does not move Git, files, or executables. Every
-- Go action runs in the configuration on the session's own host.
---@param name string
---@param run fun(ctx: rex.Context, args: table): any
---@return fun(ctx: rex.Context, args: table): any
local function on_owner(name, run)
  return function(ctx, args)
    local forwarded, result = hosts.forward(ctx, name, args)
    if forwarded then
      return result
    end
    return run(views.context(ctx), args)
  end
end

local LINE = { id = "string?", query = "string" }

function M.install()
  rex.action({
    name = "go",
    title = "Go to…",
    description = "Find a session, agent, worktree, or directory, or start work",
    category = "Navigation",
    keywords = "find jump switch session agent worktree directory start pr issue new",
    run = on_owner("go", function(ctx)
      -- The picker runs where the user was, so a start knows that repo.
      views.open({ title = "go", view = "go_view", at = ctx })
    end),
  })
  rex.action({
    name = "go_view",
    title = "Go: view",
    description = "The Go picker's rows, for dots-pick",
    category = "Navigation",
    run = on_owner("go_view", function(ctx)
      local host = hosts.current()
      local opts = { exclude = ctx.block_id, places = true }
      return views.live(
        M.view(snapshot.collect(opts), jumps.recent, host.label),
        snapshot.events(opts)
      )
    end),
  })
  rex.action({
    name = "go_to",
    title = "Go: open a row",
    category = "Navigation",
    args = { id = "string" },
    run = on_owner("go_to", M.go_to),
  })
  rex.action({
    name = "go_start_pr",
    title = "Go: review the PR",
    category = "Navigation",
    args = LINE,
    run = on_owner("go_start_pr", function(ctx, args)
      M.start(ctx, "pr", args)
    end),
  })
  rex.action({
    name = "go_start_issue",
    title = "Go: work on the issue",
    category = "Navigation",
    args = LINE,
    run = on_owner("go_start_issue", function(ctx, args)
      M.start(ctx, "issue", args)
    end),
  })
  rex.action({
    name = "go_start_new",
    title = "Go: start a new session",
    category = "Navigation",
    args = { id = "string?" },
    run = on_owner("go_start_new", function(ctx, args)
      M.start_new(ctx, args)
    end),
  })
  rex.action({
    name = "go_switch_host",
    title = "Go: switch host",
    category = "Navigation",
    args = { id = "string?" },
    run = on_owner("go_switch_host", function(ctx)
      app.perform("client.host.switch", nil, ctx.session_id, ctx.client_id)
    end),
  })
  rex.bind("cmd+p", "go")
end

return M
