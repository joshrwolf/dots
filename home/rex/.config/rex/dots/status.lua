-- Program status records (OSC 7501) as the views read them. Programs report
-- their own state; nothing here infers it.
local M = {}

---@alias dots.State "idle"|"working"|"done"|"blocked"|"error"

---@class dots.StatusRecord
---@field state dots.State
---@field id? string
---@field app? string
---@field effective_app? string
---@field kind? "permission"|"question"|"auth"
---@field progress? integer
---@field title? string
---@field msg? string
---@field updated_at? string
---@field owner? { pid: integer }

-- Most urgent first: what the user should look at before anything else.
local rank = { blocked = 1, error = 2, done = 3, working = 4, idle = 5 }

---@param record dots.StatusRecord
---@return string?
local function runtime_owner(record)
  local compact, nonce
  if record.app == "pi" and type(record.id) == "string" then
    compact, nonce = record.id:match("^pi/([a-f0-9]+)/([a-f0-9]+)$")
  end
  if not compact or #compact ~= 32 or #nonce ~= 16 then
    return nil
  end
  local session = compact:sub(1, 8)
    .. "-"
    .. compact:sub(9, 12)
    .. "-"
    .. compact:sub(13, 16)
    .. "-"
    .. compact:sub(17, 20)
    .. "-"
    .. compact:sub(21)
  return "pi-" .. session .. "-" .. nonce
end

---The record that best describes a terminal, or nil if it reported none.
---@param records dots.StatusRecord[]
---@return dots.StatusRecord?
function M.headline(records)
  local best
  for _, record in ipairs(records) do
    local r = rank[record.state]
    local root = record.id == nil or runtime_owner(record) ~= nil
    local best_root = best and (best.id == nil or runtime_owner(best) ~= nil)
    if
      r
      and (
        best == nil
        or r < rank[best.state]
        or (r == rank[best.state] and root and not best_root)
      )
    then
      best = record
    end
  end
  return best
end

---The exact runtime reported by the current terminal process, if unambiguous.
---@param records dots.StatusRecord[]
---@param pid? integer Current foreground process.
---@return string?
function M.agent_owner(records, pid)
  if not pid or pid <= 0 then
    return nil
  end
  local owner
  for _, record in ipairs(records) do
    local candidate = runtime_owner(record)
    if candidate and record.owner and record.owner.pid == pid then
      if owner and owner ~= candidate then
        return nil
      end
      owner = candidate
    end
  end
  return owner
end

---Whether a state wants the user.
---@param state dots.State
---@return boolean
function M.needs_attention(state)
  return state == "blocked" or state == "error" or state == "done"
end

---@param state dots.State
---@return integer
function M.rank(state)
  return rank[state] or 99
end

---A glyph and picker role for a state.
---@param state dots.State
---@return string glyph
---@return string role
function M.glyph(state)
  if state == "blocked" then
    return "●", "red"
  elseif state == "error" then
    return "✗", "red"
  elseif state == "done" then
    return "✓", "green"
  elseif state == "working" then
    return "◐", "yellow"
  end
  return "○", "muted"
end

return M
