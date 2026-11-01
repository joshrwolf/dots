local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local sessions = require("dots.sessions")

---A session `s` with one window per id in `windows`.
---@param windows string[]
---@return table
local function with_windows(windows)
  local list = {}
  for _, id in ipairs(windows) do
    list[#list + 1] = { window_id = id, layers = {} }
  end
  return {
    sessions = {
      {
        session_id = "s",
        label = "work",
        view = { session_id = "s", revision = 1, windows = list },
      },
    },
  }
end

return {
  ["leaving the last tab ends the session"] = function(t)
    local fake = fake_rex.install(t, with_windows({ "w1" }))
    expect.equal({ sessions.leave("s") }, { true })
    expect.equal(fake.only("session.destroy"), {
      { method = "session.destroy", args = { session_id = "s" } },
    })
  end,

  ["leaving one of several tabs keeps the session"] = function(t)
    local fake = fake_rex.install(t, with_windows({ "w1", "w2" }))
    expect.equal({ sessions.leave("s") }, { false })
    expect.equal(fake.only("session.destroy"), {})
  end,

  ["leaving a session that is gone reports why and destroys nothing"] = function(t)
    local fake = fake_rex.install(t, { sessions = {} })
    expect.equal({ sessions.leave("s") }, { false, "session not found" })
    expect.equal(fake.only("session.destroy"), {})
  end,

  ["a terminal's directory is its program's, else its shell's"] = function(t)
    local world = with_windows({ "w1" })
    world.sessions[1].terminals = {
      program = { process = { foreground = { cwd = "/src" }, child = { cwd = "/home" } } },
      silent = { process = { foreground = { name = "tui" }, child = { cwd = "/home" } } },
      unknown = { process = { foreground = { name = "tui" }, child = { name = "login" } } },
    }
    fake_rex.install(t, world)
    expect.equal(sessions.cwd("s", "program"), "/src")
    expect.equal(sessions.cwd("s", "silent"), "/home")
    expect.equal(sessions.cwd("s", "unknown"), nil)
  end,
}
