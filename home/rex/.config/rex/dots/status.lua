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

-- Most urgent first: what the user should look at before anything else.
local rank = { blocked = 1, error = 2, done = 3, working = 4, idle = 5 }

---The record that best describes a terminal, or nil if it reported none.
---@param records dots.StatusRecord[]
---@return dots.StatusRecord?
function M.headline(records)
  local best
  for _, record in ipairs(records) do
    local r = rank[record.state]
    if r and (best == nil or r < rank[best.state]) then
      best = record
    end
  end
  return best
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
