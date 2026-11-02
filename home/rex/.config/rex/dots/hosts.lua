-- Host-local actions execute in their owner's configuration, not just through
-- its session API: rex.exec and filesystem access belong to that server.
local must = require("dots.result").must
local process = require("dots.process")

local M = {}

---Delegate an action when the app is showing another server's session.
---Native routing supplies the app's connection on that host. The owner
---interprets its row arguments; no caller environment or path translation
---is applied, and failure never falls back here.
---@param ctx rex.Context
---@param action string
---@param args? table
---@return boolean forwarded
---@return any result
function M.forward(ctx, action, args)
  if not ctx.server then
    return false
  end
  local remote = rex.server(ctx.server)
  local operation = "running " .. action .. " on " .. ctx.server
  if ctx.session_id then
    must(operation, remote.call("session.attach", { session_id = ctx.session_id }))
  end
  local reply = must(
    operation,
    remote.call("client.action", {
      action = action,
      args = args,
      session_id = ctx.session_id,
      block_id = ctx.block_id,
      origin = ctx.origin == "script" and "api" or ctx.origin,
    })
  )
  for _, queued in ipairs(reply.client_actions or {}) do
    rex.client.queue(queued.action, queued.args)
  end
  return true, reply.result
end

---Identity comes from Rex; the operating system's host name is display only.
---@return { id: string, label: string }
function M.current()
  local server = must("identifying this host", rex.call("server.status"))
  if not server.host_id or server.host_id == "" then
    error("this Rex server needs a state directory to identify its host", 0)
  end
  local named = must("reading the host name", process.run({ "uname", "-n" }, { timeout = 3 }))
  if not named.ok then
    error("reading the host name: " .. named.stderr, 0)
  end
  local label = named.stdout:match("^%s*(.-)%s*$")
  if label == "" then
    error("uname returned an empty host name", 0)
  end
  return { id = server.host_id, label = label }
end

return M
