-- Pane advertisement only. Losing or removing metadata must not revoke the
-- editor endpoint or dispose of unsaved state.
local P = require("dots.herdr.protocol")
local M = {}

---@class dots.herdr.Attachment
---@field start fun(): boolean
---@field stop fun()
---@field phase fun(): string

---@param host dots.herdr.Host
---@param endpoint dots.herdr.Endpoint
---@param report fun(error: dots.herdr.Failure)
---@return dots.herdr.Attachment
function M.new(host, endpoint, report)
  local phase, pending, attempted = "idle", nil, false
  local attachment = {}
  function attachment.phase()
    return phase
  end
  function attachment.start()
    if phase == "publishing" or phase == "stopped" then
      return false
    end
    local ok, err = pcall(function()
      endpoint.identity({ registration = endpoint.registration, server = host.server })
      -- This caller-aware publisher is for directly launched editors only.
      -- A managed UI needs its own fenced pane-binding contract.
      endpoint.authorize(nil)
      phase, attempted = "publishing", true
      local handle = host.publish(endpoint.registration, function(failure)
        if phase ~= "publishing" then
          return
        end
        pending = nil
        phase = failure and "failed" or "ready"
        if failure then
          report(failure)
        end
      end)
      if phase == "publishing" then
        pending = handle
      end
    end)
    if not ok then
      phase = "failed"
      report(P.failure(err, "host_unavailable"))
    end
    return ok
  end
  function attachment.stop()
    if phase == "stopped" then
      return
    end
    phase = "stopped"
    if not attempted then
      return
    end
    if pending then
      local ok, err = pcall(pending.cancel)
      pending = nil
      if not ok then
        report(P.failure(err, "host_unavailable"))
      end
    end
    local ok, err = pcall(host.clear, endpoint.registration)
    if not ok then
      report(P.failure(err, "host_unavailable"))
    end
  end
  return attachment
end

return M
