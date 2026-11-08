-- The diff viewer beside a pane: `dots-diff` for the pane's worktree, in a
-- split to its right. Each pane has at most one; the key that opened it
-- closes it, from either pane. An agent opens or reuses the viewer beside
-- its own pane, without taking focus, with `show`.
local hosts = require("dots.hosts")
local must = require("dots.result").must
local process = require("dots.process")
local sessions = require("dots.sessions")

local M = {}

---@class dots.DiffViewer
---@field session_id string
---@field block_id string The viewer's terminal.

---Viewers by the pane they sit beside. Module state, so after a reload the
---key opens another viewer rather than closing one opened before it.
---@type table<string, dots.DiffViewer>
M.viewers = {}

---@class dots.DiffArgs
---@field show? boolean Open or reuse without taking focus; never close.
---@field path? string The worktree, or a directory in it, to show.
---@field file? string A file to scroll to: absolute, or within the worktree.

---Whether `block_id` is still placed in `view`.
---@param view rex.View
---@param block_id string
---@return boolean
local function placed(view, block_id)
  for _, window in ipairs(view.windows or {}) do
    for _, layer in ipairs(window.layers or {}) do
      for _, block in ipairs(layer.blocks or {}) do
        if block.block_id == block_id then
          return true
        end
      end
    end
  end
  return false
end

---The viewer beside `block_id`, or the one `block_id` is, with the pane it
---sits beside. Forgets viewers that have closed.
---@param view rex.View
---@param block_id string
---@return string? origin
---@return dots.DiffViewer? viewer
local function find(view, block_id)
  for origin, viewer in pairs(M.viewers) do
    if viewer.session_id == view.session_id then
      if not placed(view, viewer.block_id) then
        M.viewers[origin] = nil
      elseif origin == block_id or viewer.block_id == block_id then
        return origin, viewer
      end
    end
  end
  return nil, nil
end

---The viewer's program.
---@param dots_diff string
---@param path string
---@param file? string
---@return string[]
function M.command(dots_diff, path, file)
  local argv = { dots_diff, path }
  if file then
    argv[#argv + 1] = "--file"
    argv[#argv + 1] = file
  end
  return argv
end

---Points a running viewer at `args.path` and `args.file`.
---@param dots_diff string
---@param viewer dots.DiffViewer
---@param args dots.DiffArgs
local function steer(dots_diff, viewer, args)
  local steps = {}
  if args.path then
    steps[#steps + 1] = { "worktree", args.path }
  end
  if args.file then
    steps[#steps + 1] = { "file", args.file }
  end
  for _, step in ipairs(steps) do
    local argv = { dots_diff, "ctl", "--block", viewer.block_id, step[1], step[2] }
    local ran = must("steering the diff viewer", process.run(argv, { timeout = 10 }))
    if not ran.ok then
      error("steering the diff viewer: " .. ran.stderr, 0)
    end
  end
end

---@param ctx rex.Context
---@param args dots.DiffArgs
---@return string? block_id The viewer, or nil when the key closed it.
function M.toggle(ctx, args)
  local session_id = ctx.session_id or error("the diff viewer needs a Rex session", 0)
  local here = ctx.block_id or error("the diff viewer opens beside a pane", 0)
  local dots_diff = process.which("dots-diff")
    or error("dots-diff is not installed on the command path", 0)
  local view = must("reading the session", sessions.view(session_id))
  local origin, viewer = find(view, here)
  if origin and viewer then
    if args.show then
      steer(dots_diff, viewer, args)
      return viewer.block_id
    end
    if here == viewer.block_id then
      must(
        "focusing the pane",
        rex.session.focus_block({ session_id = session_id, block_id = origin })
      )
    end
    must(
      "closing the diff viewer",
      rex.call("block.close", { session_id = session_id, block_id = viewer.block_id })
    )
    M.viewers[origin] = nil
    return nil
  end
  local path = args.path
    or sessions.cwd(session_id, here)
    or error("the pane reports no working directory", 0)
  local created = must(
    "opening the diff viewer",
    rex.session.new_split({
      session_id = session_id,
      anchor_id = here,
      direction = "horizontal",
      side = "after",
      ratio = 0.4,
      focus = not args.show,
      layout = rex.layout.block({
        flavor = "com.superlogical.terminal.shell",
        label = "diff",
        options = { command = M.command(dots_diff, path, args.file), cwd = path },
      }),
    })
  )
  local block_id = created.block_ids and created.block_ids[1]
    or error("opening the diff viewer: Rex returned no block", 0)
  M.viewers[here] = { session_id = session_id, block_id = block_id }
  return block_id
end

function M.install()
  rex.action({
    name = "diff_toggle",
    title = "Diff: toggle",
    description = "Show the changes in this pane's worktree, and every other worktree of its repository, in a split beside it",
    category = "Navigation",
    args = { show = "boolean?", path = "string?", file = "string?" },
    run = function(ctx, args)
      local forwarded, result = hosts.forward(ctx, "diff_toggle", args)
      if forwarded then
        return result
      end
      return M.toggle(ctx, args)
    end,
  })
  rex.bind("ctrl+s>d", "diff_toggle")
end

return M
