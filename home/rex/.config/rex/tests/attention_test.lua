local attention = require("dots.attention")
local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local hosts = require("dots.hosts")
local process = require("dots.process")
local views = require("dots.views")
local snapshot = require("dots.snapshot")
local jumps = require("dots.jumps")
local sessions = require("dots.sessions")

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
  ["conversations require exact runtime identity rather than a directory"] = function()
    local snap = {
      worktrees = {},
      dirs = {},
      sessions = {
        {
          id = "s",
          label = "service-cleanup",
          blocks = {
            { id = "a", agent_owner = "pi-a-nonce" },
            { id = "b", cwd = "/src", program = "pi" },
          },
        },
      },
    }
    expect.equal(attention.conversations(snap), {
      ["pi-a-nonce"] = { state = "reported", label = "service-cleanup", session = "s", block = "a" },
    })
    snap.sessions[1].blocks[2].agent_owner = "pi-a-nonce"
    expect.equal(attention.conversations(snap), { ["pi-a-nonce"] = { state = "ambiguous" } })
  end,

  ["opening rechecks runtime and location before navigating"] = function(t)
    fake_rex.install(t, world())
    local owner = "pi-01234567-89ab-7cde-8fab-0123456789ab-0123456789abcdef"
    local current = true
    local pid = 42
    t.stub(sessions, "terminal", function(session, block, method)
      expect.equal({ session, block }, { "target", "agent" })
      if method == "process" then
        return { foreground = { pid = pid } }
      end
      return {
        records = current and {
          {
            state = "idle",
            app = "pi",
            id = "pi/0123456789ab7cde8fab0123456789ab/0123456789abcdef",
            owner = { pid = 42 },
          },
        } or {},
      }
    end)
    local snap = {
      worktrees = {},
      dirs = {},
      sessions = {
        {
          id = "target",
          label = "service-cleanup",
          blocks = { { id = "agent", agent_owner = owner } },
        },
      },
    }
    t.stub(snapshot, "collect", function()
      return snap
    end)
    local navigated = {}
    t.stub(jumps, "open_block", function(ctx, session, block)
      navigated[#navigated + 1] = { ctx = ctx, session = session, block = block }
    end)
    local actions = fake_rex.registered(t, attention)
    local ctx = { session_id = "s", block_id = "pane" }
    local args = { owner = owner, session = "target", block = "agent" }
    actions.attention_open.run(ctx, args)
    expect.equal(navigated, { { ctx = ctx, session = "target", block = "agent" } })
    current = false
    expect.fails(function()
      actions.attention_open.run(ctx, args)
    end, "does not report this runtime")
    current = true
    pid = 43
    expect.fails(function()
      actions.attention_open.run(ctx, args)
    end, "does not report this runtime")
    pid = 42
    snap.sessions[1].id = "moved"
    expect.fails(function()
      actions.attention_open.run(ctx, args)
    end, "conversation moved")
    snap.sessions[1].blocks[1].agent_owner = "pi-other-nonce"
    expect.fails(function()
      actions.attention_open.run(ctx, args)
    end, "not reported")
    expect.equal(#navigated, 1)
  end,

  ["discovery and navigation forward before reading local state"] = function(t)
    fake_rex.install(t, world())
    t.stub(snapshot, "collect", function()
      error("read local sessions")
    end)
    local sent = {}
    t.stub(hosts, "forward", function(_, action, args)
      sent[#sent + 1] = { action = action, args = args }
      return true, "forwarded"
    end)
    local actions = fake_rex.registered(t, attention)
    local ctx = { session_id = "s", block_id = "pane", server = "remote" }
    local args = { owner = "pi-a-nonce", session = "target", block = "agent" }
    expect.equal(actions.attention_sessions.run(ctx, {}), "forwarded")
    expect.equal(actions.attention_open.run(ctx, args), "forwarded")
    expect.equal(
      sent,
      { { action = "attention_sessions", args = {} }, { action = "attention_open", args = args } }
    )
  end,
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
