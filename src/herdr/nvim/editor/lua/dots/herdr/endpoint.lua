-- Local editor readiness is independent of whether any Herdr pane advertises
-- it. The editor owner, never a transient UI/metadata publisher, closes this.
local P = require("dots.herdr.protocol")
local M = {}

---@class dots.herdr.Endpoint
---@field registration string
---@field identity fun(context: dots.herdr.Context): dots.herdr.Identity
---@field close fun()
---@field authorize fun(lease: dots.herdr.LeaseId?)
---@field control fun(control: table): table?

---@param server string Herdr association, fixed for this endpoint's lifetime.
---@param listener dots.herdr.Listener Already listening.
---@param ownership? table Explicit managed lease; absence means a directly launched editor.
---@return dots.herdr.Endpoint
function M.new(server, listener, ownership)
  local closed = false
  local registration = listener.token
  return {
    registration = registration,
    identity = function(context)
      P.check(not closed, "not_attached", "editor endpoint is closed")
      P.check(
        context.registration == registration and context.server == server,
        "identity_mismatch",
        "editor registration or Herdr server changed"
      )
      return {
        protocol = P.version,
        context = { registration = registration, server = server },
        pid = vim.fn.getpid(),
      }
    end,
    authorize = function(lease)
      P.check(not closed, "not_attached", "editor endpoint is closed")
      if ownership then
        ownership.authorize(lease)
      else
        P.check(lease == nil, "not_managed", "editor is not controller-managed")
      end
    end,
    control = function(control)
      P.check(not closed, "not_attached", "editor endpoint is closed")
      if ownership then
        return ownership.control(control)
      end
      P.check(control.action == "inspect", "not_managed", "editor is not controller-managed")
    end,
    close = function()
      if not closed then
        -- Revoke dispatch even if closing the underlying listener fails.
        closed = true
        listener.close()
      end
    end,
  }
end

return M
