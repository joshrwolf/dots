-- Working with a session other than the one an action runs in. Rex answers
-- only connections attached to a session, and every `rex do` is a new
-- connection: `view` and `focus` attach first; `terminal` expects the caller
-- to have attached, as reading the session's view does.
local M = {}

local TERMINAL = "com.superlogical.terminal"

---@param session_id string
---@return table? result
---@return string? err
function M.attach(session_id)
  return rex.call("session.attach", { session_id = session_id })
end

---The session's windows, layouts, and blocks.
---@param session_id string
---@return rex.View?
---@return string? err
function M.view(session_id)
  local _, err = M.attach(session_id)
  if err then
    return nil, err
  end
  return rex.session.view({ session_id = session_id })
end

---Calls a terminal method on a block in the session, such as `process`.
---@param session_id string
---@param block_id string
---@param method string
---@param args? table
---@return table?
---@return string? err
function M.terminal(session_id, block_id, method, args)
  local call = { session_id = session_id, block_id = block_id }
  for key, value in pairs(args or {}) do
    call[key] = value
  end
  return rex.block.call(TERMINAL, method, call)
end

---The directory of a terminal's foreground program, or of its shell when the
---program reports none, or nil.
---@param session_id string
---@param block_id string
---@return string?
function M.cwd(session_id, block_id)
  if select(2, M.attach(session_id)) then
    return nil
  end
  local proc = M.terminal(session_id, block_id, "process")
  return proc and (proc.foreground and proc.foreground.cwd or proc.child and proc.child.cwd)
end

---Focuses a terminal in the session, making its window the active one.
---@param session_id string
---@param block_id string
---@return table?
---@return string? err
function M.focus(session_id, block_id)
  local _, err = M.attach(session_id)
  if err then
    return nil, err
  end
  return rex.session.focus_block({ session_id = session_id, block_id = block_id })
end

---Leaving a tab of the session, from the tab's own shell as it exits: ends
---the session when that tab is its last, which Rex would otherwise keep with
---no windows; with other tabs open, the session stays and only this tab
---closes.
---@param session_id string
---@return boolean ended
---@return string? err
function M.leave(session_id)
  local view, err = M.view(session_id)
  if not view then
    return false, err
  end
  if #(view.windows or {}) > 1 then
    return false
  end
  local _, destroyed = rex.call("session.destroy", { session_id = session_id })
  return destroyed == nil, destroyed
end

---The window holding a block, or nil when the session or block is gone.
---@param session_id string
---@param block_id string
---@return string? window_id
function M.window_of(session_id, block_id)
  local view = M.view(session_id)
  for _, window in ipairs(view and view.windows or {}) do
    for _, layer in ipairs(window.layers or {}) do
      for _, placed in ipairs(layer.blocks or {}) do
        if placed.block_id == block_id then
          return window.window_id
        end
      end
    end
  end
  return nil
end

return M
