-- What views show, read from Rex and the filesystem: every session's
-- terminals with their directories and reported status, and optionally the
-- places (worktrees, recent directories) a session could be opened in.
local must = require("dots.result").must
local process = require("dots.process")
local sessions = require("dots.sessions")
local status = require("dots.status")

local TERMINAL = "com.superlogical.terminal"

-- Where worktrunk puts worktrees under home, as `worktree-path` in
-- home/worktrunk/.config/worktrunk/config.toml says: `<root><main checkout>/<branch>`.
local WORKTREES = "/.wt"

local M = {}

---@class dots.Block
---@field id string
---@field window_id string
---@field tab integer Its window's position in the session, from 1.
---@field tab_label? string Its window's label, if someone set one.
---@field label? string Its own label, if someone set one, as a worker pane has.
---@field cwd? string
---@field program? string The foreground program as invoked, such as pi or nvim; nil at the login shell's prompt.
---@field title? string The title its program set, such as Pi's `π - dots`.
---@field focused? boolean The focused block of its window.
---@field status? dots.StatusRecord

---@class dots.Session
---@field id string
---@field label string
---@field blocks dots.Block[] In tab order, then layout order within a tab.

---@class dots.Snapshot
---@field sessions dots.Session[]
---@field worktrees string[]
---@field dirs string[]
---@field home? string

---@class dots.CollectOptions
---@field exclude? string A block to leave out, such as the picker's own.
---@field places? boolean Also list worktrees and recent directories.

---The Rex events after which a snapshot collected with `opts` may differ.
---@param opts? dots.CollectOptions
---@return string[]
function M.events(opts)
  local events = {
    "session_created",
    "session_destroyed",
    "session_label_changed",
    -- Layout, placement, and focus, including a new block once it is placed.
    "session_view_changed",
    "block_closed",
    "terminal.program_status_changed",
    "terminal.program_status_removed",
  }
  if opts and opts.places then
    -- Directories decide which places already have a session.
    events[#events + 1] = "terminal.pwd_changed"
    events[#events + 1] = "terminal.process_changed"
  end
  return events
end

---The worktree paths in `git worktree list --porcelain -z` output.
---@param porcelain string NUL-separated fields, each record ending in an empty one.
---@return string[]
function M.worktree_paths(porcelain)
  local paths = {}
  for field in porcelain:gmatch("([^%z]*)%z") do
    local path = field:match("^worktree (.+)$")
    if path then
      paths[#paths + 1] = path
    end
  end
  return paths
end

---@param argv string[]
---@param environment? table<string, string>
---@return string[] lines
local function lines(argv, environment)
  local result = process.run(argv, { timeout = 5 }, environment)
  local out = {}
  if result and result.ok then
    for line in result.stdout:gmatch("[^\n]+") do
      out[#out + 1] = line
    end
  end
  return out
end

---The directories zoxide knows, most frecent first.
---@param environment? table<string, string>
---@return string[]
function M.dirs(environment)
  return lines({ "zoxide", "query", "--list" }, environment)
end

---@param snapshot dots.Snapshot
local function add_places(snapshot)
  local environment = must("reading places", process.environment())
  local seen, cwds = {}, {}
  for _, session in ipairs(snapshot.sessions) do
    for _, block in ipairs(session.blocks) do
      if block.cwd then
        cwds[block.cwd] = true
      end
    end
  end
  local ordered = {}
  for cwd in pairs(cwds) do
    ordered[#ordered + 1] = cwd
  end
  table.sort(ordered)
  for _, cwd in ipairs(ordered) do
    -- A listing names every linked root in that repository. Subdirectories
    -- still need a query: they may belong to a nested repository.
    if not seen[cwd] then
      local listing = process.run(
        { "git", "-C", cwd, "worktree", "list", "--porcelain", "-z" },
        { timeout = 5 },
        environment
      )
      if listing and listing.ok then
        for _, path in ipairs(M.worktree_paths(listing.stdout)) do
          if not seen[path] then
            seen[path] = true
            snapshot.worktrees[#snapshot.worktrees + 1] = path
          end
        end
      end
    end
  end
  table.sort(snapshot.worktrees)
  snapshot.dirs = M.repos(M.dirs(environment), snapshot.home)
end

---The repositories among `dirs`: primary checkouts under `~/dev`, whose
---`.git` is a directory. Other places zoxide has seen (temporary, system,
---and tool directories, folders that only group repos, and subdirectories)
---are not where work starts.
---@param dirs string[]
---@param home? string
---@param exists? fun(path: string): boolean
---@return string[]
function M.repos(dirs, home, exists)
  exists = exists
    or function(path)
      local file = io.open(path, "r")
      if file then
        file:close()
      end
      return file ~= nil
    end
  local code = home and (home .. "/dev/")
  local repos = {}
  for _, dir in ipairs(dirs) do
    -- HEAD is readable only when .git is a directory, which skips linked
    -- worktrees and submodules.
    if code and dir:sub(1, #code) == code and exists(dir .. "/.git/HEAD") then
      repos[#repos + 1] = dir
    end
  end
  return repos
end

---A label someone set, or nil for the one Rex makes up: `Window <id suffix>`
---for a window and the flavor's last word, such as `shell`, for a block
---(verified on Rex 0.0.1792433367).
---@param label? string
---@param default string
---@return string?
local function chosen(label, default)
  if label == nil or label == "" or label == default then
    return nil
  end
  return label
end

---The program a terminal runs as invoked: `pi` rather than the `node` it runs
---on. A login shell is invoked as `-zsh`; at its prompt there is none.
---@param foreground? { argv0?: string, name?: string }
---@return string?
local function program(foreground)
  local argv0 = foreground and (foreground.argv0 or foreground.name)
  if not argv0 or argv0 == "" or argv0:sub(1, 1) == "-" then
    return nil
  end
  return M.basename(argv0)
end

---@param opts? dots.CollectOptions
---@return dots.Snapshot
function M.collect(opts)
  opts = opts or {}
  local listed = must("listing sessions", rex.call("session.list"))
  local snapshot = { sessions = {}, worktrees = {}, dirs = {}, home = os.getenv("HOME") }
  for _, entry in ipairs(listed.sessions or {}) do
    local view = sessions.view(entry.session_id)
      or { windows = {}, revision = 0, session_id = entry.session_id }
    local session = { id = entry.session_id, label = entry.label or entry.session_id, blocks = {} }
    for tab, window in ipairs(view.windows) do
      local tab_label = chosen(window.label, "Window " .. window.window_id:sub(-8))
      for _, layer in ipairs(window.layers or {}) do
        for _, placed in ipairs(layer.blocks or {}) do
          if placed.block_id ~= opts.exclude and placed.creator_name == TERMINAL then
            local proc = sessions.terminal(entry.session_id, placed.block_id, "process")
            local reported = sessions.terminal(entry.session_id, placed.block_id, "program_status")
            local foreground = proc and proc.foreground
            local titled = sessions.terminal(entry.session_id, placed.block_id, "title")
            local block = {
              id = placed.block_id,
              window_id = window.window_id,
              tab = tab,
              tab_label = tab_label,
              label = chosen(placed.label, (placed.flavor or ""):match("([^.]+)$") or ""),
              cwd = foreground and foreground.cwd,
              program = program(foreground),
              title = titled and titled.title ~= "" and titled.title or nil,
              focused = placed.block_id == window.focused_block_id or nil,
              status = status.headline(reported and reported.records or {}),
            }
            session.blocks[#session.blocks + 1] = block
          end
        end
      end
    end
    snapshot.sessions[#snapshot.sessions + 1] = session
  end
  if opts.places then
    add_places(snapshot)
  end
  return snapshot
end

---macOS reaches /tmp, /var, and /etc through symlinks into /private: shells
---report the short form, resolved paths the long one. Compare without it.
---@param path string
---@return string
local function comparable(path)
  for _, root in ipairs({ "/tmp", "/var", "/etc" }) do
    local long = "/private" .. root
    if path == long or path:sub(1, #long + 1) == long .. "/" then
      return path:sub(#"/private" + 1)
    end
  end
  return path
end

---The first session with a terminal in `path`, and that terminal, if any.
---@param snapshot dots.Snapshot
---@param path string
---@return string? session_id
---@return string? block_id
function M.session_at(snapshot, path)
  path = comparable(path)
  for _, session in ipairs(snapshot.sessions) do
    for _, block in ipairs(session.blocks) do
      if block.cwd and comparable(block.cwd) == path then
        return session.id, block.id
      end
    end
  end
  return nil
end

---`path` with `home` shown as `~`.
---@param path string
---@param home? string
---@return string
function M.tilde(path, home)
  if home and home ~= "" and (path == home or path:sub(1, #home + 1) == home .. "/") then
    return "~" .. path:sub(#home + 1)
  end
  return path
end

---The last component of `path`, ignoring trailing slashes.
---@param path string
---@return string
function M.basename(path)
  return path:match("([^/]+)/*$") or path
end

---`<repo>@<branch>` for a path in a worktree worktrunk made, followed by
---any directory below the worktree; nil elsewhere. The snapshot's worktrees,
---when collected, say where a worktree ends; otherwise `path` is taken to be
---one.
---@param path string
---@param snapshot { home?: string, worktrees?: string[] }
---@return string?
function M.worktree(path, snapshot)
  local home = snapshot.home
  if not home or home == "" then
    return nil
  end
  local root = home .. WORKTREES
  if path:sub(1, #root + 1) ~= root .. "/" then
    return nil
  end
  local best ---@type string?
  for _, tree in ipairs(snapshot.worktrees or {}) do
    local inside = path == tree or path:sub(1, #tree + 1) == tree .. "/"
    if inside and #tree > #root + 1 and (not best or #tree > #best) then
      best = tree
    end
  end
  local top = best or path
  local main, branch = top:sub(#root + 1):match("^(/.+)/([^/]+)/*$")
  if not main then
    return nil
  end
  local name = M.basename(main) .. "@" .. branch
  local below = path:sub(#top + 2)
  return below ~= "" and name .. "/" .. below or name
end

---A short name for a directory: its worktree as `<repo>@<branch>`, or its last component.
---@param path string
---@param snapshot { home?: string, worktrees?: string[] }
---@return string
function M.name(path, snapshot)
  return M.worktree(path, snapshot) or M.basename(path)
end

---Where a directory is, briefly: its worktree as `<repo>@<branch>`, or the path from `~`.
---@param path string
---@param snapshot { home?: string, worktrees?: string[] }
---@return string
function M.location(path, snapshot)
  return M.worktree(path, snapshot) or M.tilde(path, snapshot.home)
end

return M
