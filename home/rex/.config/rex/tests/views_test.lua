local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local process = require("dots.process")
local views = require("dots.views")

local SHELL = "com.superlogical.terminal.shell"

---@param t tests.T
local function only_dots_pick(t)
  t.stub(process, "which", function(name)
    return name == "dots-pick" and "/bin/dots-pick" or nil
  end)
end

---A session with one window: a tiled `pane` and `other`, the first zoomed when
---`zoomed`, and a floating `go` view when `open`.
---@param opts { zoomed?: boolean, open?: boolean }
---@return tests.FakeRex
local function world(opts)
  local layers = {
    { kind = "tiled", blocks = { { block_id = "pane" }, { block_id = "other" } } },
  }
  if opts.open then
    layers[2] = { kind = "floating", layer_id = "old", blocks = { { block_id = "go" } } }
  end
  return {
    sessions = {
      {
        session_id = "s",
        label = "work",
        view = {
          session_id = "s",
          revision = 1,
          windows = {
            { window_id = "w", zoomed_block_id = opts.zoomed and "pane" or nil, layers = layers },
          },
        },
        terminals = { pane = { process = { foreground = { cwd = "/src" } } } },
      },
    },
  }
end

---Views open in the world, tracked as `views.open` would have: `go`, opened
---from `pane`.
---@param t tests.T
---@param zoomed boolean
local function tracking_go(t, zoomed)
  t.stub(views, "open_views", {
    go = { title = "go", session_id = "s", layer_id = "old", origin = "pane", zoomed = zoomed },
  })
end

---@param fake tests.InstalledFake
---@return tests.Call[]
local function changes(fake)
  return fake.only(
    "session.new_layer",
    "session.close_layer",
    "session.zoom_block",
    "session.focus_block"
  )
end

return {
  ["a link is clickable and remains visible after its command exits"] = function(t)
    local fake = fake_rex.install(t, {})
    t.stub(process, "which", function(name)
      return name == "printf" and "/bin/printf" or nil
    end)
    local url = "https://github.com/example/project/pull/7"
    views.link({ session_id = "s" }, url)
    local created = fake.only("session.new_window")
    expect.equal(#created, 1)
    expect.equal(created[1].args.session_id, "s")
    local options = created[1].args.layout.block.options
    expect.equal(
      options.command,
      { "/bin/printf", "%s", "\27]8;;" .. url .. "\7" .. url .. "\27]8;;\7\n" }
    )
    expect.equal(options.shell, "none")
    expect.equal(options.login, false)
    expect.equal(options.exit, { on_completion = false })
    expect.equal(fake.only("exec"), {})
  end,

  ["link refuses terminal control characters before creating a pane"] = function(t)
    local fake = fake_rex.install(t, {})
    expect.fails(function()
      views.link({ session_id = "s" }, "https://example.test/\27]52;c;data\7")
    end, "HTTPS URL")
    expect.equal(fake.only("session.new_window"), {})
  end,

  ["link creation failures reach the caller"] = function(t)
    fake_rex.install(t, { errors = { ["session.new_window"] = "session is gone" } })
    t.stub(process, "which", function()
      return "/bin/printf"
    end)
    expect.fails(function()
      views.link({ session_id = "s" }, "https://example.test/pr")
    end, "showing the link: session is gone")
  end,

  ["scripts live beside the configuration"] = function()
    expect.equal(views.script("watch"):match("%.config/rex/scripts/watch%.lua$") ~= nil, true)
  end,

  ["a live view watches its events for as long as it is open"] = function(t)
    t.stub(process, "which", function()
      return "/bin/rex"
    end)
    local view = views.live({ accept = "go_to", groups = {}, rows = {} }, { "block_closed" })
    local watch = view.watch --[[@as string[] ]]
    expect.equal(watch[1]:match("rex$") ~= nil, true)
    expect.equal({ watch[2], watch[3], watch[4], watch[5] }, {
      "do",
      views.script("watch"),
      "--all",
      "--args",
    })
    expect.equal(rex.json.decode(watch[6]), { events = { "block_closed" } })
    expect.equal(#watch, 6) -- no time limit: the picker stops it
    expect.equal(view.preview_every, 1)
  end,

  ["open floats the picker over the pane's window, in its directory"] = function(t)
    only_dots_pick(t)
    t.stub(views, "open_views", {})
    local fake = fake_rex.install(t, world({ zoomed = true }))
    expect.equal(
      views.open({ title = "go", view = "go_view", at = { session_id = "s", block_id = "pane" } }),
      "layer"
    )
    expect.equal(changes(fake), {
      {
        method = "session.new_layer",
        args = {
          session_id = "s",
          window_id = "w",
          bounds = { 0.04, 0.05, 0.92, 0.88 },
          focus = true,
          layout = rex.layout.block({
            flavor = SHELL,
            label = "go",
            options = {
              command = { "/bin/dots-pick", "go_view", "--dismiss", "views_dismiss" },
              cwd = "/src",
            },
          }),
        },
      },
    })
    expect.equal(views.open_views, {
      picker = {
        title = "go",
        session_id = "s",
        layer_id = "layer",
        origin = "pane",
        zoomed = true,
      },
    })
  end,

  ["a view opened from another replaces it and keeps where both return"] = function(t)
    only_dots_pick(t)
    tracking_go(t, true)
    local fake = fake_rex.install(t, world({ open = true }))
    views.open({ title = "prs", view = "prs_view", at = { session_id = "s", block_id = "go" } })
    local calls = changes(fake)
    expect.equal(
      calls[1],
      { method = "session.close_layer", args = { session_id = "s", layer_id = "old" } }
    )
    expect.equal(calls[2].method, "session.new_layer")
    expect.equal(
      calls[2].args.layout,
      rex.layout.block({
        flavor = SHELL,
        label = "prs",
        options = {
          command = { "/bin/dots-pick", "prs_view", "--dismiss", "views_dismiss" },
          cwd = "/src",
        },
      })
    )
    expect.equal(#calls, 2)
    expect.equal(views.open_views, {
      picker = {
        title = "prs",
        session_id = "s",
        layer_id = "layer",
        origin = "pane",
        zoomed = true,
      },
    })
  end,

  ["a view's own key closes it and returns to the pane, zoomed again"] = function(t)
    only_dots_pick(t)
    tracking_go(t, true)
    local fake = fake_rex.install(t, world({ open = true }))
    expect.equal(
      views.open({ title = "go", view = "go_view", at = { session_id = "s", block_id = "go" } }),
      nil
    )
    expect.equal(changes(fake), {
      {
        method = "session.zoom_block",
        args = { session_id = "s", block_id = "pane", zoom = true },
      },
      { method = "session.close_layer", args = { session_id = "s", layer_id = "old" } },
    })
    expect.equal(views.open_views, {})
  end,

  ["opening a view from a pane closes the window's other view"] = function(t)
    only_dots_pick(t)
    tracking_go(t, false)
    local fake = fake_rex.install(t, world({ open = true }))
    views.open({ title = "go", view = "go_view", at = { session_id = "s", block_id = "other" } })
    local calls = changes(fake)
    expect.equal(
      calls[1],
      { method = "session.close_layer", args = { session_id = "s", layer_id = "old" } }
    )
    expect.equal(calls[2].method, "session.new_layer")
    expect.equal(views.open_views.picker.origin, "other")
  end,

  ["dismissing a view returns to its pane, zoomed again if it was"] = function(t)
    for _, zoomed in ipairs({ true, false }) do
      tracking_go(t, zoomed)
      local fake = fake_rex.install(t, world({ open = true }))
      views.dismiss({ session_id = "s", block_id = "go" })
      local restored = zoomed
          and {
            method = "session.zoom_block",
            args = { session_id = "s", block_id = "pane", zoom = true },
          }
        or { method = "session.focus_block", args = { session_id = "s", block_id = "pane" } }
      expect.equal(changes(fake), { restored })
      expect.equal(views.open_views, {})
    end
  end,

  ["dismissing leaves focus to Rex for an unknown view or a closed pane"] = function(t)
    t.stub(views, "open_views", {})
    local fake = fake_rex.install(t, world({ open = true }))
    views.dismiss({ session_id = "s", block_id = "go" })
    expect.equal(fake.calls, {})

    t.stub(views, "open_views", {
      go = { title = "go", session_id = "s", layer_id = "old", origin = "gone", zoomed = true },
    })
    fake = fake_rex.install(t, world({ open = true }))
    views.dismiss({ session_id = "s", block_id = "go" })
    expect.equal(changes(fake), {})
    expect.equal(views.open_views, {})
  end,

  ["open refuses when the picker is not installed"] = function(t)
    t.stub(process, "which", function()
      return nil
    end)
    local fake = fake_rex.install(t, world({}))
    expect.fails(function()
      views.open({ title = "go", view = "go_view", at = { session_id = "s", block_id = "pane" } })
    end, "dots-pick is not installed on the command path")
    expect.equal(fake.calls, {})
  end,

  ["open reports a layer Rex refuses"] = function(t)
    only_dots_pick(t)
    t.stub(views, "open_views", {})
    local refusing = world({})
    refusing.errors = { ["session.new_layer"] = "no session" }
    fake_rex.install(t, refusing)
    expect.fails(function()
      views.open({ title = "go", view = "go_view", at = { session_id = "s", block_id = "pane" } })
    end, "opening go: no session")
  end,
}
