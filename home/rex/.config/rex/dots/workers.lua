-- A Pi worker's saved transcript as a view. The parent Pi asks for it; a
-- minimal Pi that loads only the companion's viewer extension draws it, read
-- only, from the session file the worker writes.
local process = require("dots.process")
local views = require("dots.views")

local M = {}

---@param name string
---@param value any
---@return string
local function absolute(name, value)
  if type(value) ~= "string" or value:sub(1, 1) ~= "/" then
    error("workers_view needs an absolute " .. name, 0)
  end
  return value
end

---The viewer's program: Pi with nothing but the viewer, so it cannot run a
---model, tools, MCP servers, project files or another session. Rex terminals take no environment, so
---`env` sets the one variable that keeps it out of program status.
---@param session_file string
---@param viewer string The viewer extension.
---@return string[]
function M.command(session_file, viewer)
  local env = process.which("env") or error("env is not on the command path", 0)
  local pi = process.which("pi") or error("pi is not installed on the command path", 0)
  return {
    env,
    "PI_PROGRAM_STATUS=0",
    pi,
    "--no-session",
    "--no-extensions",
    "-e",
    viewer,
    "--no-skills",
    "--no-prompt-templates",
    "--no-context-files",
    "--no-mcp",
    "--no-tools",
    "--no-approve",
    "--offline",
    "--worker-session",
    session_file,
  }
end

function M.install()
  rex.action({
    name = "workers_view",
    title = "Workers: view transcript",
    description = "Show a Pi worker's saved transcript in a view, for the Pi that owns it",
    args = { session_file = "string", viewer = "string", label = "string" },
    run = function(ctx, args)
      local command =
        M.command(absolute("session_file", args.session_file), absolute("viewer", args.viewer))
      local label = type(args.label) == "string" and args.label ~= "" and args.label or "worker"
      return views.open({ title = "worker: " .. label, command = command, at = ctx })
    end,
  })
end

return M
