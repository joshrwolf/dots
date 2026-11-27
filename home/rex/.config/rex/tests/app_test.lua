local app = require("dots.app")
local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")

local clients = {
  { client_id = "cli", info = { kind = "cli" } },
  { client_id = "phone", info = { kind = "app" }, session_ids = { "s2" } },
  { client_id = "mac", info = { kind = "app" }, session_ids = { "s1" } },
}

return {
  ["the opening app wins over another app attached to the same session"] = function()
    expect.equal(app.choose(clients, "s2", "mac"), "mac")
    expect.equal(app.choose(clients, "s1", "cli"), "mac")
  end,

  ["a disappeared opening app never sends its action to another device"] = function()
    expect.equal(app.choose(clients, "s2", "gone"), nil)
  end,

  ["prefers the app showing the session"] = function()
    expect.equal(app.choose(clients, "s1"), "mac")
    expect.equal(app.choose(clients, "s2"), "phone")
  end,

  ["falls back to the first app, never a CLI"] = function()
    expect.equal(app.choose(clients, "elsewhere"), "phone")
    expect.equal(app.choose({ clients[1] }, "s1"), nil)
  end,

  ["a URL opens in the app's browser"] = function(t)
    local fake = fake_rex.install(t, { clients = clients })
    app.open_url("https://github.com/example/project/pull/7", "s1")
    expect.equal(fake.only("client.perform"), {
      {
        method = "client.perform",
        args = {
          client_id = "mac",
          action = "client.url.open",
          args = { url = "https://github.com/example/project/pull/7" },
        },
      },
    })
  end,

  ["only HTTPS URLs without spaces or control characters open"] = function(t)
    local fake = fake_rex.install(t, { clients = clients })
    for _, url in ipairs({
      "http://example.test/",
      "file:///etc/passwd",
      "https://example.test/a b",
      "https://example.test/\27]52;c;data\7",
    }) do
      expect.fails(function()
        app.open_url(url)
      end, "HTTPS")
    end
    expect.equal(fake.only("client.list", "client.perform"), {})
  end,
}
