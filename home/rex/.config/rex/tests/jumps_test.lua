local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local jumps = require("dots.jumps")
local hosts = require("dots.hosts")

local TERMINAL = "com.superlogical.terminal"

---@param session_id string
---@param window_id string
---@return dots.Location
local function at(session_id, window_id)
  return { session_id = session_id, window_id = window_id }
end

---@param list dots.Location[]
---@return string[]
local function keys(list)
  local out = {}
  for _, entry in ipairs(list) do
    out[#out + 1] = jumps.key(entry)
  end
  return out
end

---A session whose windows each hold one terminal named after the window.
---@param session_id string
---@param windows string[]
---@param active? string
---@return tests.FakeSession
local function session(session_id, windows, active)
  local view = { session_id = session_id, revision = 1, active_window_id = active, windows = {} }
  for _, window_id in ipairs(windows) do
    view.windows[#view.windows + 1] = {
      window_id = window_id,
      layers = {
        { kind = "tiled", blocks = { { block_id = window_id .. "-b", creator_name = TERMINAL } } },
      },
    }
  end
  return { session_id = session_id, label = session_id, view = view }
end

---@param session_id string
---@param window_id string
---@return tests.Call
local function shows(session_id, window_id)
  return {
    method = "client.perform",
    args = {
      client_id = "app",
      action = "session.select",
      args = { session_id = session_id, window_id = window_id },
    },
  }
end

return {
  ["jump back uses the selected host's history without touching this host's"] = function(t)
    t.stub(jumps, "recent", { at("local-session", "local-window") })
    t.stub(hosts, "forward", function(ctx, action)
      expect.equal(ctx.server, "studio")
      expect.equal(action, "jump_back")
      return true
    end)
    jumps.toggle({ server = "studio", session_id = "remote-session" })
    expect.equal(jumps.recent, { at("local-session", "local-window") })
  end,

  ["push puts a location first, once, and keeps fifty"] = function()
    local list = jumps.push({ at("s", "a"), at("s", "b") }, at("s", "b"))
    expect.equal(keys(list), { "s|b", "s|a" })
    for i = 1, 60 do
      list = jumps.push(list, at("s", "w" .. i))
    end
    expect.equal(#list, 50)
    expect.equal(list[1], at("s", "w60"))
    expect.equal(list[50], at("s", "w11"))
  end,

  ["push leaves the old list alone"] = function()
    local old = { at("s", "a") }
    jumps.push(old, at("s", "b"))
    expect.equal(keys(old), { "s|a" })
  end,

  ["choose skips the current tab and prunes only what is gone"] = function()
    local list = { at("s", "here"), at("s", "closed"), at("s", "unknown"), at("s", "live") }
    local alive = { ["s|closed"] = false, ["s|live"] = true }
    local target, gone = jumps.choose(list, at("s", "here"), function(location)
      return alive[jumps.key(location)]
    end)
    expect.equal(target, at("s", "live"))
    expect.equal(gone, { ["s|closed"] = true })
    expect.equal(keys(jumps.without(list, gone)), { "s|here", "s|unknown", "s|live" })
  end,

  ["choose finds nothing when only the current tab is left"] = function()
    local target = jumps.choose({ at("s", "here") }, at("s", "here"), function()
      return true
    end)
    expect.equal(target, nil)
  end,

  ["order puts recorded items first, most recent first, then the rest"] = function()
    local items = { "s|a", "s|b", "s|c", "s|d" }
    local ordered = jumps.order(items, function(item)
      return item
    end, { at("s", "c"), at("gone", "x"), at("s", "a") })
    expect.equal(ordered, { "s|c", "s|a", "s|b", "s|d" })
  end,

  ["here is the window holding the action's block"] = function(t)
    fake_rex.install(t, { sessions = { session("s", { "w1", "w2" }, "w1") } })
    expect.equal(jumps.here({ session_id = "s", block_id = "w2-b" }), at("s", "w2"))
    expect.equal(jumps.here({ session_id = "s", block_id = "elsewhere" }), at("s", "w1"))
    expect.equal(jumps.here({}), nil)
  end,

  ["record lands a windowless jump at the session's active window"] = function(t)
    t.stub(jumps, "recent", {})
    fake_rex.install(t, { sessions = { session("s2", { "w7" }, "w7") } })
    jumps.record(at("s1", "w1"), { session_id = "s2" })
    expect.equal(keys(jumps.recent), { "s2|w7", "s1|w1" })
    jumps.record(nil, { session_id = "missing" })
    expect.equal(keys(jumps.recent), { "s2|w7", "s1|w1" })
  end,

  ["toggle goes back, and again returns"] = function(t)
    t.stub(jumps, "recent", { at("s2", "w2"), at("s1", "w1") })
    local fake = fake_rex.install(t, {
      sessions = { session("s1", { "w1" }, "w1"), session("s2", { "w2" }, "w2") },
    })
    jumps.toggle({ session_id = "s2", block_id = "w2-b" })
    jumps.toggle({ session_id = "s1", block_id = "w1-b" })
    expect.equal(fake.only("client.perform"), { shows("s1", "w1"), shows("s2", "w2") })
    expect.equal(keys(jumps.recent), { "s2|w2", "s1|w1" })
  end,

  ["toggle first records a tab reached another way"] = function(t)
    t.stub(jumps, "recent", { at("s1", "w1"), at("s2", "w2") })
    local fake = fake_rex.install(t, {
      sessions = { session("s1", { "w1" }, "w1"), session("s2", { "w2", "w3" }, "w3") },
    })
    -- The sidebar moved the user to w3 after jumping to w1.
    jumps.toggle({ session_id = "s2", block_id = "w3-b" })
    expect.equal(fake.only("client.perform"), { shows("s1", "w1") })
    expect.equal(keys(jumps.recent), { "s1|w1", "s2|w3", "s2|w2" })
  end,

  ["toggle forgets closed tabs and sessions"] = function(t)
    t.stub(jumps, "recent", { at("s1", "closed"), at("gone", "w"), at("s1", "w1") })
    local fake = fake_rex.install(t, { sessions = { session("s1", { "w1", "w2" }, "w2") } })
    jumps.toggle({ session_id = "s1", block_id = "w2-b" })
    expect.equal(fake.only("client.perform"), { shows("s1", "w1") })
    expect.equal(keys(jumps.recent), { "s1|w1", "s1|w2" })
  end,

  ["toggle with nowhere to go shows nothing"] = function(t)
    t.stub(jumps, "recent", {})
    local fake = fake_rex.install(t, { sessions = { session("s1", { "w1" }, "w1") } })
    jumps.toggle({ session_id = "s1", block_id = "w1-b" })
    expect.equal(fake.only("client.perform"), {})
    expect.equal(keys(jumps.recent), { "s1|w1" })
  end,

  ["cmd+o toggles"] = function(t)
    local actions, bindings = fake_rex.registered(t, jumps)
    expect.equal(bindings, { { key = "cmd+o", target = "jump_back" } })
    expect.equal(actions.jump_back.run, jumps.toggle)
  end,
}
