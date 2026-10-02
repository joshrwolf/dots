-- A managed editor fences automation when its controller stops renewing. This
-- never closes a UI, buffer, listener, or process. Local editing remains usable.
local P = require("dots.herdr.protocol")
local lease_ms = 5000
local M = { lease_ms = lease_ms }

---@param owner string Fresh 128-bit controller-run identifier, not a PID.
---@param now? fun(): number Monotonic milliseconds; narrow test clock seam.
function M.new(owner, now)
  P.check(P.token(owner), "invalid_request", "invalid initial owner")
  now = now or function()
    return math.floor(vim.uv.hrtime() / 1000000)
  end
  local generation, phase, recovered_from = 1, "active", nil
  local expires = now() + lease_ms
  local function observe()
    local at = now()
    if phase == "active" and at >= expires then
      phase = "orphaned"
    end
    return at
  end
  local function matches(lease)
    return lease and lease.owner == owner and lease.generation == generation
  end
  local function require_current(lease)
    P.check(lease, "owner_required", "managed navigation requires an owner lease")
    P.check(matches(lease), "owner_mismatch", "owner generation changed; inspect current ownership")
  end
  local ownership = {}
  function ownership.status()
    local at = observe()
    return {
      lease = { owner = owner, generation = generation },
      phase = phase,
      remaining_ms = phase == "active" and math.max(0, expires - at) or 0,
    }
  end
  function ownership.authorize(lease)
    local at = observe()
    require_current(lease)
    P.check(phase == "active", "owner_expired", "editor is orphaned; explicit recovery is required")
    return at
  end
  function ownership.control(control)
    observe()
    if control.action == "inspect" then
      return ownership.status()
    end
    P.fresh(control.not_after_ms)
    if control.action == "renew" then
      local at = ownership.authorize(control.lease)
      expires = at + lease_ms
    elseif control.action == "release" then
      require_current(control.lease)
      phase = "orphaned"
    elseif control.action == "recover" then
      -- A lost recovery acknowledgement may be inspected/retried without
      -- extending the lease or resurrecting an already expired successor.
      if
        recovered_from
        and recovered_from.owner == control.expected.owner
        and recovered_from.generation == control.expected.generation
        and owner == control.owner
      then
        return ownership.status()
      end
      require_current(control.expected)
      P.check(phase == "orphaned", "owner_active", "cannot replace a live owner")
      P.check(
        control.owner ~= owner and generation < 4294967295,
        "invalid_request",
        "recovery requires a new owner and an available generation"
      )
      recovered_from = { owner = owner, generation = generation }
      owner, generation, phase = control.owner, generation + 1, "active"
      expires = now() + lease_ms
    else
      P.raise("invalid_request", "unknown owner operation")
    end
    return ownership.status()
  end
  return ownership
end

return M
