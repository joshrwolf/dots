local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local prs = require("dots.prs")
local process = require("dots.process")
local snapshots = require("tests.fixtures.snapshots")
local watch = require("tests.fixtures.watch")

local function by_id(view)
  local rows = {}
  for _, row in ipairs(view.rows) do
    rows[row.id] = row
  end
  return rows
end

local function url(number)
  return "https://github.com/example/project/pull/" .. number
end

---A Rex with `sessions` (default none), and the commands PR actions run:
---listing `watch.mixed`, and finding only PR 2's checkout.
---@param t tests.T
---@param sessions? tests.FakeSession[]
---@return tests.InstalledFake
local function install(t, sessions)
  t.stub(process, "which", function(name)
    return "/bin/" .. name
  end)
  return fake_rex.install(t, {
    sessions = sessions,
    commands = {
      ["dots github watch list --json"] = { stdout = rex.json.encode(watch.mixed) },
      ["test -d /home/me/src/dots"] = { ok = true },
      ["test -d /gone"] = { ok = false },
      ["dots github watch remove " .. url(3)] = {},
      ["dots github watch remove " .. url(4)] = { ok = false, stderr = "not watched" },
    },
  })
end

---@param fake tests.InstalledFake
---@return string[]
local function commands(fake)
  local out = {}
  for _, call in ipairs(fake.only("exec")) do
    out[#out + 1] = table.concat(call.args, " ")
  end
  return out
end

local ctx = { session_id = "s", block_id = "picker" }

return {
  ["groups put what needs you first and finished PRs last"] = function()
    local rows = by_id(prs.view(watch.mixed, snapshots.empty, "/bin/dots"))
    local groups = {}
    for number = 1, 8 do
      groups[number] = rows[url(number)].group
    end
    expect.equal(groups, {
      "open",
      "attention",
      "attention",
      "ready",
      "open",
      "attention",
      "done",
      "done",
    })
  end,

  ["every row has one cell per column"] = function()
    local view = prs.view(watch.mixed, snapshots.busy, "/bin/dots")
    for _, row in ipairs(view.rows) do
      expect.equal(#row.cells, 5)
    end
  end,

  ["names the session working at each PR's checkout"] = function()
    local rows = by_id(prs.view(watch.mixed, snapshots.busy, "/bin/dots"))
    expect.equal(rows[url(2)].cells[1], { text = "dots", role = "blue" })
    expect.equal(rows[url(6)].cells[1].text, "") -- its checkout has no session
    expect.equal(rows[url(1)].cells[1].text, "") -- it has no checkout
    expect.equal(rows[url(2)].search:find(" dots ", 1, true) ~= nil, true)
  end,

  ["a PR names the live session its work belongs to over its checkout's"] = function()
    ---@type dots.WatchList
    local list = {
      due = 0,
      prs = {
        {
          url = url(1),
          repository = "example/project",
          number = 1,
          state = "open",
          review = "none",
          ci = "none",
          checkout = "/home/me/src/dots",
          rex_session = "session:two",
          added_at = 1,
        },
      },
    }
    local label = snapshots.busy.sessions[2].label
    local row = prs.view(list, snapshots.busy, "/bin/dots").rows[1]
    expect.equal(row.cells[1], { text = label, role = "blue" })
    expect.equal(row.search:find(label, 1, true) ~= nil, true)
    -- Without a checkout, the recorded session alone names it.
    local rows = by_id(prs.view(watch.mixed, snapshots.busy, "/bin/dots"))
    expect.equal(rows[url(3)].cells[1].text, label)
  end,

  ["a PR whose recorded session is gone names its checkout's session"] = function()
    local rows = by_id(prs.view(watch.mixed, snapshots.busy, "/bin/dots"))
    expect.equal(watch.mixed.prs[2].rex_session, "session:gone")
    expect.equal(rows[url(2)].cells[1].text, "dots")
  end,

  ["a PR names whichever session works at its checkout"] = function()
    ---@type dots.WatchList
    local list = {
      due = 0,
      prs = {
        {
          url = url(1),
          repository = "example/project",
          number = 1,
          state = "open",
          review = "none",
          ci = "none",
          checkout = "/home/me/src/with space\tand tab",
          added_at = 1,
        },
      },
    }
    local label = snapshots.busy.sessions[2].label
    local row = prs.view(list, snapshots.busy, "/bin/dots").rows[1]
    expect.equal(row.cells[1].text, label)
    expect.equal(row.search:find(label, 1, true) ~= nil, true)
  end,

  ["shows only the ten most recent merged and closed PRs"] = function()
    local list = { due = 0, prs = {} } ---@type dots.WatchList
    for number = 1, 14 do
      list.prs[number] = {
        url = url(number),
        repository = "example/project",
        number = number,
        state = number % 2 == 0 and "merged" or "closed",
        review = "none",
        ci = "none",
        added_at = 100 + number,
        -- Odd PRs were checked after every even one was added.
        checked_at = number % 2 == 1 and 200 + number or nil,
      }
    end
    list.prs[15] = {
      url = url(15),
      repository = "example/project",
      number = 15,
      state = "open",
      review = "none",
      ci = "none",
      added_at = 1,
    }
    local ids = {}
    for _, row in ipairs(prs.view(list, snapshots.empty, "/bin/dots").rows) do
      ids[#ids + 1] = row.id
    end
    expect.equal(ids, {
      url(15),
      url(13),
      url(11),
      url(9),
      url(7),
      url(5),
      url(3),
      url(1),
      url(14),
      url(12),
      url(10),
    })
  end,

  ["status shows errors and closure over CI"] = function()
    local rows = by_id(prs.view(watch.mixed, snapshots.empty, "/bin/dots"))
    expect.equal(rows[url(6)].cells[4].text, "! error")
    expect.equal(rows[url(7)].cells[4].text, "merged")
    expect.equal(rows[url(2)].cells[4].text, "✗ ci")
    expect.equal(rows[url(5)].cells[5].text, "draft")
  end,

  ["a PR not checked yet says so instead of a title"] = function()
    local rows = by_id(prs.view(watch.mixed, snapshots.empty, "/bin/dots"))
    expect.equal(rows[url(1)].cells[3], { text = "not checked yet", role = "muted" })
    expect.equal(rows[url(1)].cells[2].text, "project#1")
  end,

  ["previews and the watch read the local store through dots"] = function()
    local view = prs.view(watch.mixed, snapshots.empty, "/bin/dots")
    expect.equal(view.watch, { "/bin/dots", "github", "watch", "follow" })
    expect.equal(
      view.rows[1].preview,
      { "/bin/dots", "github", "watch", "show", "--color", view.rows[1].id }
    )
    expect.equal(view.preview_every, nil)
    expect.equal(view.preview_anchor, "start") -- a PR reads from its title down
    expect.equal(view.preview_position, "below") -- rows keep their columns
  end,

  ["open shows the live session the PR's work belongs to"] = function(t)
    local fake = install(t, { { session_id = "session:two", label = "two" } })
    prs.open(ctx, { id = url(3) })
    expect.equal(commands(fake), { "dots github watch list --json" })
    expect.equal(fake.only("session.create"), {})
    local performed = fake.only("client.perform")
    expect.equal(#performed, 1)
    expect.equal(performed[1].args.action, "session.select")
    expect.equal(performed[1].args.args.session_id, "session:two")
  end,

  ["open goes to the checkout's session"] = function(t)
    -- PR 2 records a session Rex no longer has.
    local fake = install(t)
    prs.open(ctx, { id = url(2) })
    expect.equal(commands(fake), { "dots github watch list --json", "test -d /home/me/src/dots" })
    expect.equal(#fake.only("session.create"), 1)
    expect.equal(fake.only("session.create")[1].args.label, "dots")
    expect.equal(#fake.only("client.perform"), 1)
  end,

  ["open shows a link for a PR whose checkout is gone or unknown"] = function(t)
    local fake = install(t)
    prs.open(ctx, { id = url(6) })
    prs.open(ctx, { id = url(1) })
    expect.equal(commands(fake), {
      "dots github watch list --json",
      "test -d /gone",
      "dots github watch list --json",
    })
    expect.equal(fake.only("session.create", "client.perform"), {})
    local links = fake.only("session.new_window")
    expect.equal(#links, 2)
    expect.equal(links[1].args.layout.block.options.command[3]:find(url(6), 1, true) ~= nil, true)
    expect.equal(links[2].args.layout.block.options.command[3]:find(url(1), 1, true) ~= nil, true)
  end,

  ["link and unwatch act on the selected PR"] = function(t)
    local actions = fake_rex.registered(t, prs)
    local fake = install(t)
    actions.prs_link.run(ctx, { id = url(1) })
    actions.prs_unwatch.run(ctx, { id = url(3) })
    expect.equal(commands(fake), { "dots github watch remove " .. url(3) })
    expect.equal(#fake.only("session.new_window"), 1)
    expect.fails(function()
      actions.prs_unwatch.run(ctx, { id = url(4) })
    end, "dots github watch remove: not watched")
  end,
}
