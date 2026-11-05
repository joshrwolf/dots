-- Attention: what goals in every registered project need from the user, and
-- their work, in one popup. `dots attention view` owns the store, the
-- presentation and the answers; this module only opens it as a view.
local hosts = require("dots.hosts")
local process = require("dots.process")
local views = require("dots.views")

local M = {}

---The viewer across every registered project.
---@param dots string The dots command.
---@return string[]
function M.command(dots)
  return { dots, "attention", "view", "--scope", "global" }
end

function M.install()
  rex.action({
    name = "attention",
    title = "Attention…",
    description = "Decisions and actions goals need from you, and their work, across projects",
    category = "Navigation",
    keywords = "attention inbox requests questions decisions answer goals tasks work",
    run = function(ctx)
      local forwarded, result = hosts.forward(ctx, "attention", {})
      if forwarded then
        return result
      end
      local dots = process.which("dots") or error("dots is not installed on the command path", 0)
      views.open({ title = "attention", command = M.command(dots), at = ctx })
    end,
  })
  rex.bind("cmd+i", "attention")
end

return M
