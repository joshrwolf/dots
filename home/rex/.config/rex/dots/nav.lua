-- Ctrl+H/J/K/L move between panes, except where the pane's program uses them:
-- Vim-family editors navigate their own splits and hand focus back at an edge
-- (see the Neovim keymaps), and pickers use Ctrl+J/K to move their selection.
local sessions = require("dots.sessions")

---@class dots.Nav
local M = {}

---@alias dots.Direction "left"|"down"|"up"|"right"

---@param name string
---@return boolean
function M.is_vim(name)
  local base = name:lower():gsub("%.exe$", "")
  if base:find("^vim%.[%w_]+$") then
    return true
  end
  for _, editor in ipairs({
    "vi",
    "vim",
    "vimdiff",
    "view",
    "ex",
    "rvim",
    "rview",
    "evim",
    "eview",
    "nvi",
    "nvim",
    "nvimdiff",
  }) do
    if base == editor then
      return true
    end
  end
  return false
end

---@param name string
---@return boolean
function M.is_picker(name)
  local base = name:lower()
  return base == "fzf" or base == "dots-pick"
end

---Whether a direction key belongs to the program in the pane.
---An unknown program moves focus: a key that does nothing reads as broken,
---while an unwanted focus move is undone with the opposite key.
---@param direction dots.Direction
---@param program? string The foreground process name.
---@return "forward"|"focus"
function M.decide(direction, program)
  if program == nil then
    return "focus"
  end
  if M.is_vim(program) then
    return "forward"
  end
  if M.is_picker(program) and (direction == "up" or direction == "down") then
    return "forward"
  end
  return "focus"
end

---@param direction dots.Direction
---@return rex.KeyFn
local function navigate(direction)
  return function(ctx)
    local process = sessions.terminal(ctx.session_id, ctx.block_id, "process")
    local program = process and process.foreground and process.foreground.name
    if M.decide(direction, program) == "forward" then
      return false
    end
    local _, err = rex.session.focus_direction({ direction = direction })
    if err then
      rex.log("warn", "focus " .. direction .. ": " .. err)
    end
    return true
  end
end

function M.install()
  rex.bind("ctrl+h", navigate("left"))
  rex.bind("ctrl+j", navigate("down"))
  rex.bind("ctrl+k", navigate("up"))
  rex.bind("ctrl+l", navigate("right"))
end

return M
