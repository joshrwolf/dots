local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local process = require("dots.process")
local views = require("dots.views")
local workers = require("dots.workers")

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

local args = {
  session_file = "/sessions/w-1/session.jsonl",
  viewer = "/pi/extensions/worker-view/index.ts",
  label = "review",
}

return {
  ["a worker opens as a view running only the read-only viewer"] = function(t)
    t.stub(views, "open_views", {})
    local fake = fake_rex.install(t, world())
    local actions, bindings = fake_rex.registered(t, workers)
    expect.equal(bindings, {})
    actions.workers_view.run({ session_id = "s", block_id = "pane" }, args)
    local created = fake.only("session.new_layer")
    expect.equal(#created, 1)
    expect.equal(created[1].args.bounds, views.LIST)
    expect.equal(created[1].args.layout.block.label, "worker: review")
    expect.equal(created[1].args.layout.block.options.cwd, "/src")
    expect.equal(created[1].args.layout.block.options.command, {
      "/bin/env",
      "PI_PROGRAM_STATUS=0",
      "/bin/pi",
      "--no-session",
      "--no-extensions",
      "-e",
      "/pi/extensions/worker-view/index.ts",
      "--no-skills",
      "--no-prompt-templates",
      "--no-context-files",
      "--no-mcp",
      "--no-tools",
      "--no-approve",
      "--offline",
      "--worker-session",
      "/sessions/w-1/session.jsonl",
      "--dismiss",
      "views_dismiss",
    })
    expect.equal(views.open_views.picker.title, "worker: review")
  end,

  ["relative or missing paths open nothing"] = function(t)
    local fake = fake_rex.install(t, world())
    local actions = fake_rex.registered(t, workers)
    local ctx = { session_id = "s", block_id = "pane" }
    expect.fails(function()
      actions.workers_view.run(ctx, { session_file = "session.jsonl", viewer = args.viewer })
    end, "absolute session_file")
    expect.fails(function()
      actions.workers_view.run(ctx, { session_file = args.session_file })
    end, "absolute viewer")
    expect.equal(fake.only("session.new_layer"), {})
  end,

  ["the view refuses to open without pi"] = function(t)
    local fake = fake_rex.install(t, world())
    t.stub(process, "which", function(name)
      return name ~= "pi" and "/bin/" .. name or nil
    end)
    local actions = fake_rex.registered(t, workers)
    expect.fails(function()
      actions.workers_view.run({ session_id = "s", block_id = "pane" }, args)
    end, "pi is not installed")
    expect.equal(fake.only("session.new_layer"), {})
  end,
}
