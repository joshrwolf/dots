-- Jumps: the tabs dots navigation has moved between, most recent first, so
-- Cmd+O can go back to the last one, and pressing it again returns, like
-- Vim's Ctrl-^. Features that move the user to another tab record the jump
-- here; Cmd+O also records where it starts, which catches moves made outside
-- dots. Stepping through tabs is not a jump. The list is module state, so it
-- starts empty after a reload.
local app = require("dots.app")
local hosts = require("dots.hosts")
local must = require("dots.result").must
local sessions = require("dots.sessions")

---@class dots.Jumps
local M = {}

local LIMIT = 50

---A tab: a session's window.
---@class dots.Location
---@field session_id string
---@field window_id string

---Where a jump goes. Without a window it lands on the session's active one.
---@class dots.Destination
---@field session_id string
---@field window_id? string

---Most recent first. Replace it, never change it in place, so a caller
---holding an older list is unaffected.
---@type dots.Location[]
M.recent = {}

---@param location dots.Location
---@return string
function M.key(location)
  return location.session_id .. "|" .. location.window_id
end

---`list` with `location` first and no other copy of it, at most 50 long.
---@param list dots.Location[]
---@param location dots.Location
---@return dots.Location[]
function M.push(list, location)
  local out, key = { location }, M.key(location)
  for _, entry in ipairs(list) do
    if #out >= LIMIT then
      break
    end
    if M.key(entry) ~= key then
      out[#out + 1] = entry
    end
  end
  return out
end

---`list` without the locations whose keys are in `gone`.
---@param list dots.Location[]
---@param gone table<string, boolean>
---@return dots.Location[]
function M.without(list, gone)
  local out = {}
  for _, entry in ipairs(list) do
    if not gone[M.key(entry)] then
      out[#out + 1] = entry
    end
  end
  return out
end

---The most recent location other than `current` that still exists, and the
---keys of those passed over because they no longer do. `alive` answers true,
---false for gone, or nil when it cannot tell, which skips without pruning.
---@param list dots.Location[]
---@param current? dots.Location
---@param alive fun(location: dots.Location): boolean?
---@return dots.Location? target
---@return table<string, boolean> gone
function M.choose(list, current, alive)
  local here, gone = current and M.key(current), {}
  for _, entry in ipairs(list) do
    local key = M.key(entry)
    if key ~= here then
      local live = alive(entry)
      if live then
        return entry, gone
      elseif live == false then
        gone[key] = true
      end
    end
  end
  return nil, gone
end

---`items` with recorded ones first, most recent first, then the rest in
---their order.
---@generic T
---@param items T[]
---@param key fun(item: T): string
---@param list dots.Location[]
---@return T[]
function M.order(items, key, list)
  local rank = {}
  for i, entry in ipairs(list) do
    rank[M.key(entry)] = i
  end
  local recorded, rest = {}, {}
  for _, item in ipairs(items) do
    if rank[key(item)] then
      recorded[#recorded + 1] = item
    else
      rest[#rest + 1] = item
    end
  end
  table.sort(recorded, function(a, b)
    return rank[key(a)] < rank[key(b)]
  end)
  for _, item in ipairs(rest) do
    recorded[#recorded + 1] = item
  end
  return recorded
end

---The tab an action runs in: the window holding `ctx.block_id`, or the
---session's active window. Nil when Rex cannot say.
---@param ctx rex.Context
---@return dots.Location?
function M.here(ctx)
  if not ctx.session_id then
    return nil
  end
  local view = sessions.view(ctx.session_id)
  if not view then
    return nil
  end
  for _, window in ipairs(view.windows or {}) do
    for _, layer in ipairs(window.layers or {}) do
      for _, placed in ipairs(layer.blocks or {}) do
        if placed.block_id == ctx.block_id then
          return { session_id = ctx.session_id, window_id = window.window_id }
        end
      end
    end
  end
  local active = view.active_window_id
  return active and { session_id = ctx.session_id, window_id = active } or nil
end

---Focus one existing terminal and record the jump on the viewing client.
---@param ctx rex.Context
---@param session_id string
---@param block_id string
function M.open_block(ctx, session_id, block_id)
  local from = M.here(ctx)
  must("focusing the terminal", sessions.focus(session_id, block_id))
  local window = sessions.window_of(session_id, block_id)
  app.show(session_id, window, ctx.session_id, ctx.client_id)
  M.record(from, { session_id = session_id, window_id = window })
end

---Record a jump from `from` to `to`. A destination without a window is
---recorded at its session's active window, when Rex reports one.
---@param from? dots.Location
---@param to dots.Destination|dots.Location
function M.record(from, to)
  local window = to.window_id
  if not window then
    local view = sessions.view(to.session_id)
    window = view and view.active_window_id
  end
  local recent = M.recent
  if from then
    recent = M.push(recent, from)
  end
  if window then
    recent = M.push(recent, { session_id = to.session_id, window_id = window })
  end
  M.recent = recent
end

---Whether each location still exists, from Rex's session list and views.
---@return fun(location: dots.Location): boolean?
local function liveness()
  local listed = must("listing sessions", rex.call("session.list"))
  local live, windows = {}, {}
  for _, entry in ipairs(listed.sessions or {}) do
    live[entry.session_id] = true
  end
  return function(location)
    if not live[location.session_id] then
      return false
    end
    local known = windows[location.session_id]
    if not known then
      local view = sessions.view(location.session_id)
      if not view then
        return nil
      end
      known = {}
      for _, window in ipairs(view.windows or {}) do
        known[window.window_id] = true
      end
      windows[location.session_id] = known
    end
    return known[location.window_id] == true
  end
end

---Go to the most recent other tab that still exists, forgetting those that
---do not.
---@param ctx rex.Context
function M.toggle(ctx)
  if hosts.forward(ctx, "jump_back") then
    return
  end
  local here = M.here(ctx)
  if here and not (M.recent[1] and M.key(M.recent[1]) == M.key(here)) then
    M.recent = M.push(M.recent, here)
  end
  local target, gone = M.choose(M.recent, here, liveness())
  M.recent = M.without(M.recent, gone)
  if not target then
    return
  end
  app.show(target.session_id, target.window_id, ctx.session_id, ctx.client_id)
  M.record(here, target)
end

function M.install()
  rex.action({
    name = "jump_back",
    title = "Go to the last tab",
    description = "Toggle between this tab and the last one you jumped from",
    category = "Navigation",
    keywords = "jump back previous last recent toggle alternate",
    run = M.toggle,
  })
  rex.bind("cmd+o", "jump_back")
end

return M
