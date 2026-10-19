local T, child = require("support").child_set()
local eq = MiniTest.expect.equality
local function setup()
  child.lua_func(function()
    context = require("support").context()
    fake = require("fake_host").new(context)
    warnings = {}
    endpoint = require("dots.herdr.endpoint").new(context.server, fake.listen())
    attachment = require("dots.herdr.attachment").new(fake.host, endpoint, function(err)
      table.insert(warnings, err.code)
    end)
  end)
end

T["local readiness does not wait for pane publication"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.start()
      return { attachment.phase(), endpoint.identity(context).context, #fake.publications }
    end),
    { "publishing", require("support").context(), 1 }
  )
  eq(
    child.lua_func(function()
      fake.publications[1].commit()
      fake.publications[1].deliver()
      return { attachment.phase(), endpoint.identity(context).context }
    end),
    { "ready", require("support").context() }
  )
end

T["failed registration retries with once-only completions"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.start()
      fake.publications[1].deliver({ code = "host_unavailable", message = "offline" })
      local failed = attachment.phase()
      attachment.start()
      local pending = attachment.phase()
      fake.publications[2].commit()
      fake.publications[2].deliver()
      return { failed, pending, attachment.phase(), warnings }
    end),
    { "failed", "publishing", "ready", { "host_unavailable" } }
  )
end

T["late callbacks cannot resurrect a stopped attachment"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.start()
      attachment.stop()
      fake.publications[1].deliver()
      local after_callback = attachment.phase()
      attachment.stop()
      return { after_callback, attachment.phase(), attachment.start(), fake.events }
    end),
    { "stopped", "stopped", false, { "cancel", "clear" } }
  )
end

T["shutdown settles an in-flight publication before clearing it"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.start()
      fake.publications[1].on_cancel = fake.publications[1].commit
      attachment.stop()
      fake.publications[1].deliver()
      return { fake.pane.registration == nil, fake.events, attachment.phase() }
    end),
    { true, { "cancel", "clear" }, "stopped" }
  )
end

T["failed metadata cleanup does not revoke the editor endpoint"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.start()
      fake.clear_error = true
      attachment.stop()
      return { fake.events, warnings, endpoint.identity(context).context }
    end),
    { { "cancel", "clear" }, { "host_unavailable" }, require("support").context() }
  )
end

T["pane moves and host outages do not change local editor identity"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.start()
      fake.publications[1].commit()
      fake.publications[1].deliver()
      fake.pane = { pane = "w2:p9", tab = "w2:t3", workspace = "w2" }
      fake.unavailable = true
      return endpoint.identity(context).context
    end),
    require("support").context()
  )
end

T["real listeners preserve existing endpoints and close idempotently"] = function()
  eq(
    child.lua_func(function()
      local before = vim.fn.serverlist()
      local listener = require("dots.herdr.listener").open(vim.env.HERDR_NVIM_SCRATCH .. "/s-" .. vim.fn.getpid())
      local valid = #listener.token == 32 and vim.uv.fs_lstat(listener.socket).type == "socket"
      listener.close()
      listener.close()
      return { valid, vim.uv.fs_lstat(listener.socket) == nil, vim.deep_equal(before, vim.fn.serverlist()) }
    end),
    { true, true, true }
  )
end

T["listener failures are not reported as Herdr outages"] = function()
  eq(
    child.lua_func(function()
      local path = test_root .. "/not-a-directory"
      vim.fn.writefile({ "file" }, path)
      local ok, err = pcall(require("dots.herdr.listener").open, path)
      return { ok, require("dots.herdr.protocol").failure(err).code }
    end),
    { false, "listener_unavailable" }
  )
end

T["an idle advertisement has nothing to clear"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.stop()
      return { #fake.events, endpoint.identity(context).context }
    end),
    { 0, require("support").context() }
  )
end

T["closed endpoints cannot be published"] = function()
  setup()
  eq(
    child.lua_func(function()
      endpoint.close()
      return { attachment.start(), #fake.publications, warnings }
    end),
    { false, 0, { "not_attached" } }
  )
end

T["failed publication leaves local dispatch available"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.start()
      fake.publications[1].deliver({ code = "host_unavailable", message = "offline" })
      local dispatch = require("dots.herdr.bridge").new(endpoint)
      return {
        attachment.phase(),
        vim.json.decode(dispatch(vim.json.encode({
          protocol = 1,
          context = context,
          operation = { method = "hello" },
        }))).status,
      }
    end),
    { "failed", "ready" }
  )
end

T["server association is fixed even when a publisher changes"] = function()
  setup()
  eq(
    child.lua_func(function()
      fake.host.server = "/tmp/replacement-herdr.sock"
      local ok = attachment.start()
      attachment.stop()
      return { ok, #fake.publications, warnings, endpoint.identity(context).context, #fake.events }
    end),
    { false, 0, { "identity_mismatch" }, require("support").context(), 0 }
  )
end

T["closing an endpoint revokes identity even if listener cleanup fails"] = function()
  setup()
  eq(
    child.lua_func(function()
      local endpoint = require("dots.herdr.endpoint").new(context.server, {
        token = context.registration,
        close = function()
          error("synthetic close failure")
        end,
      })
      local closed = pcall(endpoint.close)
      local ready, err = pcall(endpoint.identity, context)
      endpoint.close() -- terminal and idempotent, not resurrected by cleanup errors
      return { closed, ready, err.code }
    end),
    { false, false, "not_attached" }
  )
end

return T
