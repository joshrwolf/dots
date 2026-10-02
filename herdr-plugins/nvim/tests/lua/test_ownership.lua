local T, child = require("support").child_set()
local eq = MiniTest.expect.equality

local function setup()
  child.lua_func(function()
    P = require("dots.herdr.protocol")
    Owner = require("dots.herdr.ownership")
    context = require("support").context()
    a, b, c = string.rep("a", 32), string.rep("b", 32), string.rep("c", 32)
    initial = { owner = a, generation = 1 }
    clock, closed = 0, false
    ownership = Owner.new(a, function()
      return clock
    end)
    listener = {
      token = context.registration,
      close = function()
        closed = true
      end,
    }
    endpoint = require("dots.herdr.endpoint").new(context.server, listener, ownership)
    dispatch = require("dots.herdr.bridge").new(endpoint)
    path = test_root .. "/target.txt"
    vim.fn.writefile({ "target", "second" }, path)
    original = vim.api.nvim_get_current_buf()
    vim.api.nvim_buf_set_lines(original, 0, -1, false, { "unsaved work" })
    function request(operation)
      return vim.json.decode(dispatch(vim.json.encode({ protocol = 1, context = context, operation = operation })))
    end
    function control(action, fields)
      local value = vim.tbl_extend("force", { action = action, not_after_ms = P.now_ms() + 2000 }, fields or {})
      if action == "inspect" then
        value.not_after_ms = nil
      end
      return request({ method = "owner", control = value })
    end
    function open(lease)
      return request({
        method = "open",
        target = { path = path, line = 1, column = 1 },
        not_after_ms = P.now_ms() + 2000,
        lease = lease,
      })
    end
  end)
end

T["expiry rejects both navigation and late renewal without touching editor state"] = function()
  setup()
  eq(
    child.lua_func(function()
      local missing = open().error.code
      local before = vim.api.nvim_list_bufs()
      clock = Owner.lease_ms
      local expired = open(initial)
      local renew = control("renew", { lease = initial })
      local state = control("inspect").ownership
      return {
        missing,
        expired.error.code,
        renew.error.code,
        state.phase,
        state.remaining_ms,
        closed,
        vim.api.nvim_get_current_buf() == original,
        vim.bo.modified,
        vim.deep_equal(before, vim.api.nvim_list_bufs()),
      }
    end),
    { "owner_required", "owner_expired", "owner_expired", "orphaned", 0, false, true, true, true }
  )
end

T["only timely renewal extends the monotonic lease"] = function()
  setup()
  eq(
    child.lua_func(function()
      clock = Owner.lease_ms - 1
      local renewed = control("renew", { lease = initial }).ownership
      clock = Owner.lease_ms
      local active = control("inspect").ownership.phase
      clock = 2 * Owner.lease_ms - 1
      return { renewed.remaining_ms, active, control("inspect").ownership.phase }
    end),
    { 5000, "active", "orphaned" }
  )
end

T["release is idempotent and does not dispose of the editor"] = function()
  setup()
  eq(
    child.lua_func(function()
      local first = control("release", { lease = initial }).ownership
      local second = control("release", { lease = initial }).ownership
      return { first, second, closed, vim.api.nvim_buf_get_lines(original, 0, -1, false), vim.bo[original].modified }
    end),
    {
      { lease = { owner = string.rep("a", 32), generation = 1 }, phase = "orphaned", remaining_ms = 0 },
      { lease = { owner = string.rep("a", 32), generation = 1 }, phase = "orphaned", remaining_ms = 0 },
      false,
      { "unsaved work" },
      true,
    }
  )
end

T["recovery fences competing recoveries and every stale owner operation"] = function()
  setup()
  eq(
    child.lua_func(function()
      clock = Owner.lease_ms
      local recovered = control("recover", { expected = initial, owner = b }).ownership
      local competing = control("recover", { expected = initial, owner = c })
      local stale = {
        open(initial).error.code,
        control("renew", { lease = initial }).error.code,
        control("release", { lease = initial }).error.code,
        competing.error.code,
      }
      return { recovered.lease.generation, stale, open(recovered.lease).status, vim.bo[original].modified }
    end),
    { 2, { "owner_mismatch", "owner_mismatch", "owner_mismatch", "owner_mismatch" }, "opened", true }
  )
end

T["matching owner ID with the wrong generation grants no authority"] = function()
  setup()
  eq(
    child.lua_func(function()
      local wrong = { owner = a, generation = 2 }
      local replies = {
        open(wrong),
        control("renew", { lease = wrong }),
        control("release", { lease = wrong }),
        control("recover", { expected = wrong, owner = b }),
      }
      local codes = {}
      for _, reply in ipairs(replies) do
        table.insert(codes, reply.error and reply.error.code or "accepted")
      end
      return codes
    end),
    { "owner_mismatch", "owner_mismatch", "owner_mismatch", "owner_mismatch" }
  )
end

T["live owners cannot be stolen or extended by a failed recovery"] = function()
  setup()
  eq(
    child.lua_func(function()
      clock = 1000
      local before = control("inspect").ownership
      local result = control("recover", { expected = initial, owner = b })
      return { result.error.code, vim.deep_equal(before, control("inspect").ownership) }
    end),
    { "owner_active", true }
  )
end

T["replaying recovery acknowledges but never renews or resurrects its successor"] = function()
  setup()
  eq(
    child.lua_func(function()
      control("release", { lease = initial })
      successor = control("recover", { expected = initial, owner = b }).ownership.lease
      clock = 1000
      return control("recover", { expected = initial, owner = b }).ownership.remaining_ms
    end),
    4000
  )
  eq(
    child.lua_func(function()
      clock = Owner.lease_ms
      local expired = control("recover", { expected = initial, owner = b }).ownership
      local late = control("renew", { lease = successor })
      return { expired.phase, expired.remaining_ms, late.error and late.error.code or "accepted" }
    end),
    { "orphaned", 0, "owner_expired" }
  )
  eq(
    child.lua_func(function()
      local third = control("recover", { expected = successor, owner = c }).ownership
      local old = control("recover", { expected = initial, owner = b })
      return { third.lease.generation, old.error.code }
    end),
    { 3, "owner_mismatch" }
  )
end

T["queued owner mutations"] = MiniTest.new_set({ parametrize = { { "renew" }, { "release" }, { "recover" } } })
T["queued owner mutations"]["must not execute after their request deadline"] = function(action)
  setup()
  eq(
    child.lua_func(function(action)
      local before = control("inspect").ownership
      local fields = action == "recover" and { expected = initial, owner = b } or { lease = initial }
      fields.not_after_ms = 1
      local result = control(action, fields)
      return { result.error.code, vim.deep_equal(before, control("inspect").ownership) }
    end, action),
    { "expired", true }
  )
end

T["lease loss in a buffer hook is checked again before presentation"] = function()
  setup()
  eq(
    child.lua_func(function()
      vim.api.nvim_create_autocmd("BufReadPost", {
        once = true,
        callback = function()
          clock = Owner.lease_ms
        end,
      })
      local result = open(initial)
      return { result.status, result.error.code, vim.api.nvim_get_current_buf() == original }
    end),
    { "rejected", "owner_expired", true }
  )
end

T["unmanaged editors cannot be silently converted by control requests"] = function()
  setup()
  eq(
    child.lua_func(function()
      endpoint = require("dots.herdr.endpoint").new(context.server, listener)
      dispatch = require("dots.herdr.bridge").new(endpoint)
      return {
        control("inspect").ownership == vim.NIL,
        control("recover", { expected = initial, owner = b }).error.code,
        open(initial).error.code,
        open().status,
      }
    end),
    { true, "not_managed", "not_managed", "opened" }
  )
end

T["managed editors cannot use the direct-editor pane publisher"] = function()
  setup()
  eq(
    child.lua_func(function()
      local fake = require("fake_host").new(context)
      local warnings = {}
      local publisher = require("dots.herdr.attachment").new(fake.host, endpoint, function(err)
        table.insert(warnings, err.code)
      end)
      local started = publisher.start()
      publisher.stop()
      return { started, #fake.publications, #fake.events, warnings }
    end),
    { false, 0, 0, { "owner_required" } }
  )
end

T["recovery requires a fresh controller-run identity"] = function()
  setup()
  eq(
    child.lua_func(function()
      clock = Owner.lease_ms
      local result = control("recover", { expected = initial, owner = a })
      return { result.error.code, control("inspect").ownership.lease.generation }
    end),
    { "invalid_request", 1 }
  )
end

T["closed endpoints cannot be recovered"] = function()
  setup()
  eq(
    child.lua_func(function()
      endpoint.close()
      return control("recover", { expected = initial, owner = b }).error.code
    end),
    "not_attached"
  )
end

T["real elapsed time expires ownership without a timer callback"] = function()
  setup()
  eq(
    child.lua_func(function()
      local owner = Owner.new(a)
      vim.wait(Owner.lease_ms + 50)
      local ok, err = pcall(owner.authorize, initial)
      return { ok, err.code, owner.status().phase, vim.bo[original].modified }
    end),
    { false, "owner_expired", "orphaned", true }
  )
end

return T
