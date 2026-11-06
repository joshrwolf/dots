-- Central goal inspection; opening it never follows, claims or starts work.
local hosts = require("dots.hosts")
local process = require("dots.process")
local views = require("dots.views")

local M = {}

---@param dots string
---@param args { project?: string, goal?: string }
---@return string[]
function M.command(dots, args)
  if (args.project ~= nil) ~= (args.goal ~= nil) then
    error("Select an exact project and goal together, or neither", 0)
  end
  local command = { dots, "task", "goal", "view" }
  if args.project then
    command[#command + 1] = "--select"
    command[#command + 1] = args.project .. ":" .. args.goal
  end
  return command
end

function M.install()
  rex.action({
    name = "goals",
    title = "Goals…",
    description = "Browse goals, dependency graphs, briefs and history across registered projects",
    category = "Navigation",
    args = { project = "string?", goal = "string?" },
    run = function(ctx, args)
      local forwarded, result = hosts.forward(ctx, "goals", args)
      if forwarded then
        return result
      end
      local dots = process.which("dots") or error("dots is not installed on the command path", 0)
      return views.open({ title = "goals", command = M.command(dots, args), at = ctx })
    end,
  })
  rex.bind("cmd+g", "goals")
end

return M
