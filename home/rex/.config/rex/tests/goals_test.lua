local goals = require("dots.goals")
local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local hosts = require("dots.hosts")
local process = require("dots.process")
local views = require("dots.views")

return {
  ["global entry and exact preselection use the same browser"] = function()
    expect.equal(goals.command("/bin/dots", {}), { "/bin/dots", "task", "goal", "view" })
    expect.equal(goals.command("/bin/dots", { project = "p-fixture", goal = "t-123abc" }), {
      "/bin/dots",
      "task",
      "goal",
      "view",
      "--select",
      "p-fixture:t-123abc",
    })
    expect.fails(function()
      goals.command("/bin/dots", { goal = "t-123abc" })
    end, "together")
  end,
  ["Cmd G opens the native shared view without starting Pi"] = function(t)
    fake_rex.install(t, { sessions = {} })
    t.stub(hosts, "forward", function()
      return false
    end)
    t.stub(process, "which", function()
      return "/bin/dots"
    end)
    local opened
    t.stub(views, "open", function(spec)
      opened = spec
      return "layer"
    end)
    local actions, bindings = fake_rex.registered(t, goals)
    local ctx = { session_id = "s", block_id = "b" }
    expect.equal(bindings, { { key = "cmd+g", target = "goals" } })
    expect.equal(actions.goals.run(ctx, { project = "p-fixture", goal = "t-123abc" }), "layer")
    expect.equal(opened, {
      title = "goals",
      at = ctx,
      command = { "/bin/dots", "task", "goal", "view", "--select", "p-fixture:t-123abc" },
    })
  end,
  ["remote routing happens before local command resolution"] = function(t)
    fake_rex.install(t, { sessions = {} })
    t.stub(hosts, "forward", function(ctx, action, args)
      expect.equal(
        { ctx.server, action, args },
        { "remote", "goals", { project = "p-fixture", goal = "t-123abc" } }
      )
      return true, "remote-layer"
    end)
    t.stub(process, "which", function()
      error("resolved locally")
    end)
    local actions = fake_rex.registered(t, goals)
    expect.equal(
      actions.goals.run({ server = "remote" }, { project = "p-fixture", goal = "t-123abc" }),
      "remote-layer"
    )
  end,
  ["unavailable hosts and commands never launch a replacement"] = function(t)
    fake_rex.install(t, { sessions = {} })
    t.stub(hosts, "forward", function()
      return false
    end)
    t.stub(process, "which", function()
      return nil
    end)
    t.stub(views, "open", function()
      error("opened a view without dots")
    end)
    local actions = fake_rex.registered(t, goals)
    expect.fails(function()
      actions.goals.run({}, {})
    end, "dots is not installed")
    t.stub(hosts, "forward", function()
      error("remote unavailable")
    end)
    expect.fails(function()
      actions.goals.run({}, {})
    end, "remote unavailable")
  end,
}
