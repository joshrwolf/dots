-- `rex do` connects to whichever Rex server is running; there is no offline
-- mode. Tests must never reach it, so every call that could read or change a
-- live session fails loudly unless a test's fake stands in for it.
local M = {}

---@param name string
---@return fun(): nil
local function refuse(name)
  return function()
    error("tests must not reach Rex: " .. name .. " has no fake", 0)
  end
end

function M.install()
  ---@diagnostic disable: duplicate-set-field -- replaced for the whole run
  rex.call = refuse("rex.call")
  rex.block.call = refuse("rex.block.call")
  rex.invoke = refuse("rex.invoke")
  rex.server = refuse("rex.server")
  rex.servers = refuse("rex.servers")
  for name, value in pairs(rex.session) do
    if type(value) == "function" then
      rex.session[name] = refuse("rex.session." .. name)
    end
  end
  ---@diagnostic enable: duplicate-set-field
end

return M
