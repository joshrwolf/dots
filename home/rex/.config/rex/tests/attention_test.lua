local attention = require("dots.attention")
local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local hosts = require("dots.hosts")
local process = require("dots.process")
local views = require("dots.views")

---@return tests.FakeRex
local function world()
  return {
    sessions = {
      {
        session_id = "s",
        label = "work",
        view = {
          session_id = "s",
          revision = 1,
          windows = {
            {
              window_id = "w",
              layers = { { kind = "tiled", blocks = { { block_id = "pane" } } } },
            },
          },
        },
        terminals = { pane = { process = { foreground = { cwd = "/src" } } } },
      },
    },
  }
end

return {
  ["the key opens the global viewer as a view that closes through views_dismiss"] = function(t)
    t.stub(views, "open_views", {})
    local fake = fake_rex.install(t, world())
    local actions, bindings = fake_rex.registered(t, attention)
    expect.equal(bindings, { { key = "cmd+i", target = "attention" } })
    actions.attention.run({ session_id = "s", block_id = "pane" }, {})
    local created = fake.only("session.new_layer")
    expect.equal(#created, 1)
    expect.equal(created[1].args.bounds, views.LIST)
    expect.equal(created[1].args.layout.block.label, "attention")
    expect.equal(created[1].args.layout.block.options.command, {
      "/bin/dots",
      "attention",
      "view",
      "--scope",
      "global",
      "--dismiss",
      "views_dismiss",
    })
    expect.equal(views.open_views.picker.title, "attention")
  end,

  ["remote attention resolves commands and opens only on its owning host"] = function(t)
    fake_rex.install(t, world())
    local ctx = { session_id = "s", block_id = "pane", server = "remote" }
    local forwarded = {}
    t.stub(hosts, "forward", function(at, action, args)
      forwarded = { at = at, action = action, args = args }
      return true, "remote-layer"
    end)
    t.stub(process, "which", function()
      error("resolved a command on the caller's host")
    end)
    t.stub(views, "open", function()
      error("opened a view on the caller's host")
    end)
    local actions = fake_rex.registered(t, attention)
    expect.equal(actions.attention.run(ctx, {}), "remote-layer")
    expect.equal(forwarded, { at = ctx, action = "attention", args = {} })
  end,

  ["remote attention failure never falls back to local state"] = function(t)
    local fake = fake_rex.install(t, world())
    t.stub(hosts, "forward", function()
      error("remote attention unavailable")
    end)
    t.stub(process, "which", function()
      error("resolved a command on the caller's host")
    end)
    local actions = fake_rex.registered(t, attention)
    expect.fails(function()
      actions.attention.run({ session_id = "s", block_id = "pane", server = "remote" }, {})
    end, "remote attention unavailable")
    expect.equal(fake.only("session.new_layer"), {})
  end,

  ["the viewer refuses to open without dots"] = function(t)
    local fake = fake_rex.install(t, world())
    t.stub(process, "which", function()
      return nil
    end)
    local actions = fake_rex.registered(t, attention)
    expect.fails(function()
      actions.attention.run({ session_id = "s", block_id = "pane" }, {})
    end, "dots is not installed on the command path")
    expect.equal(fake.only("session.new_layer"), {})
  end,
}
