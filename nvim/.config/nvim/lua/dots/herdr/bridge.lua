-- Dispatch only. Protocol validation and editor mutation have separate tests.
local P = require("dots.herdr.protocol")
local Editor = require("dots.herdr.editor")
local M = {}

---@param endpoint dots.herdr.Endpoint
---@return fun(json: string): string
function M.new(endpoint)
  return function(json)
    local ok, reply = pcall(function()
      local request = P.decode(json)
      local identity = endpoint.identity(request.context)
      if request.operation.method == "hello" then
        return { status = "ready", identity = identity }
      end
      if request.operation.method == "owner" then
        return {
          status = "ownership",
          identity = identity,
          ownership = endpoint.control(request.operation.control) or vim.NIL,
        }
      end
      local function revalidate()
        identity = endpoint.identity(request.context)
        P.fresh(request.operation.not_after_ms)
        endpoint.authorize(request.operation.lease)
      end
      revalidate()
      local partial, observed = Editor.open(request.operation.target, revalidate)
      if partial then
        return { status = "partial", error = partial }
      end
      return { status = "opened", identity = identity, target = observed }
    end)
    return vim.json.encode(ok and reply or { status = "rejected", error = P.failure(reply) })
  end
end

return M
