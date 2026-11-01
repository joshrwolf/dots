local app = require("dots.app")
local expect = require("tests.expect")

local clients = {
  { client_id = "cli", info = { kind = "cli" } },
  { client_id = "phone", info = { kind = "app" }, session_ids = { "s2" } },
  { client_id = "mac", info = { kind = "app" }, session_ids = { "s1" } },
}

return {
  ["prefers the app showing the session"] = function()
    expect.equal(app.choose(clients, "s1"), "mac")
    expect.equal(app.choose(clients, "s2"), "phone")
  end,

  ["falls back to the first app, never a CLI"] = function()
    expect.equal(app.choose(clients, "elsewhere"), "phone")
    expect.equal(app.choose({ clients[1] }, "s1"), nil)
  end,
}
