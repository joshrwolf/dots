local diff = require("dots.diff")
local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local process = require("dots.process")

---A session with one pane at /src, and the viewer `split` beside it when
---`open` says so.
---@param open? boolean
---@return tests.FakeRex
local function world(open)
  local blocks = { { block_id = "pane" } }
  if open then
    blocks[2] = { block_id = "split" }
  end
  return {
    sessions = {
      {
        session_id = "s",
        label = "work",
        view = {
          session_id = "s",
          revision = 1,
          windows = { { window_id = "w", layers = { { kind = "tiled", blocks = blocks } } } },
        },
        terminals = { pane = { process = { foreground = { cwd = "/src" } } } },
      },
    },
    commands = {
      ["/bin/dots-diff ctl --block split worktree /wt"] = {},
      ["/bin/dots-diff ctl --block split file /wt/a.rs"] = {},
      ["/bin/dots-diff ctl --block split bind pane"] = {},
    },
  }
end

local pane = { session_id = "s", block_id = "pane" }

return {
  ["the key opens the viewer for the pane's worktree beside it, focused"] = function(t)
    t.stub(diff, "viewers", {})
    local fake = fake_rex.install(t, world())
    local actions, bindings = fake_rex.registered(t, diff)
    expect.equal(bindings, { { key = "ctrl+s>d", target = "diff_toggle" } })
    expect.equal(actions.diff_toggle.run(pane, {}), "split")
    local split = fake.only("session.new_split")
    expect.equal(#split, 1)
    expect.equal(split[1].args.anchor_id, "pane")
    expect.equal(split[1].args.direction, "horizontal")
    expect.equal(split[1].args.side, "after")
    expect.equal(split[1].args.focus, true)
    expect.equal(split[1].args.layout.block.label, "diff")
    expect.equal(
      split[1].args.layout.block.options,
      { command = { "/bin/dots-diff", "/src", "--bind", "pane" }, cwd = "/src" }
    )
    expect.equal(diff.viewers, { pane = { session_id = "s", block_id = "split" } })
  end,

  ["the key closes the viewer from the pane or from the viewer"] = function(t)
    for _, from in ipairs({ "pane", "split" }) do
      t.stub(diff, "viewers", { pane = { session_id = "s", block_id = "split" } })
      local fake = fake_rex.install(t, world(true))
      local actions = fake_rex.registered(t, diff)
      expect.equal(actions.diff_toggle.run({ session_id = "s", block_id = from }, {}), nil)
      expect.equal(fake.only("session.new_split"), {})
      expect.equal(fake.only("block.close")[1].args, { session_id = "s", block_id = "split" })
      local focused = fake.only("session.focus_block")
      expect.equal(#focused, from == "split" and 1 or 0)
      if from == "split" then
        expect.equal(focused[1].args.block_id, "pane")
      end
      expect.equal(diff.viewers, {})
    end
  end,

  ["show opens a viewer without taking focus, at the given worktree and file"] = function(t)
    t.stub(diff, "viewers", {})
    local fake = fake_rex.install(t, world())
    local actions = fake_rex.registered(t, diff)
    actions.diff_toggle.run(pane, { show = true, path = "/wt", file = "a.rs" })
    local split = fake.only("session.new_split")[1].args
    expect.equal(split.focus, false)
    expect.equal(split.layout.block.options, {
      command = { "/bin/dots-diff", "/wt", "--bind", "pane", "--file", "a.rs" },
      cwd = "/wt",
    })
  end,

  ["show reuses the open viewer and points and binds it here"] = function(t)
    t.stub(diff, "viewers", { pane = { session_id = "s", block_id = "split" } })
    local fake = fake_rex.install(t, world(true))
    local actions = fake_rex.registered(t, diff)
    expect.equal(
      actions.diff_toggle.run(pane, { show = true, path = "/wt", file = "/wt/a.rs" }),
      "split"
    )
    expect.equal(fake.only("session.new_split", "block.close", "session.focus_block"), {})
    local ran = fake.only("exec")
    expect.equal(ran[1].args, { "/bin/dots-diff", "ctl", "--block", "split", "worktree", "/wt" })
    expect.equal(ran[2].args, { "/bin/dots-diff", "ctl", "--block", "split", "bind", "pane" })
    expect.equal(ran[3].args, { "/bin/dots-diff", "ctl", "--block", "split", "file", "/wt/a.rs" })
  end,

  ["a viewer that exited is forgotten and the key opens a new one"] = function(t)
    t.stub(diff, "viewers", { pane = { session_id = "s", block_id = "gone" } })
    local fake = fake_rex.install(t, world())
    local actions = fake_rex.registered(t, diff)
    expect.equal(actions.diff_toggle.run(pane, {}), "split")
    expect.equal(fake.only("block.close"), {})
    expect.equal(#fake.only("session.new_split"), 1)
  end,

  ["a failed steer is reported"] = function(t)
    t.stub(diff, "viewers", { pane = { session_id = "s", block_id = "split" } })
    local fake = world(true)
    fake.commands["/bin/dots-diff ctl --block split worktree /wt"] =
      { ok = false, stderr = "no dots-diff" }
    fake_rex.install(t, fake)
    local actions = fake_rex.registered(t, diff)
    expect.fails(function()
      actions.diff_toggle.run(pane, { show = true, path = "/wt" })
    end, "no dots-diff")
  end,

  ["nothing opens without dots-diff"] = function(t)
    t.stub(diff, "viewers", {})
    local fake = fake_rex.install(t, world())
    t.stub(process, "which", function(name)
      return name ~= "dots-diff" and "/bin/" .. name or nil
    end)
    local actions = fake_rex.registered(t, diff)
    expect.fails(function()
      actions.diff_toggle.run(pane, {})
    end, "dots-diff is not installed")
    expect.equal(fake.only("session.new_split"), {})
  end,
}
