local expect = require("tests.expect")
local hosts = require("dots.hosts")
local process = require("dots.process")

return {
  ["the current host never probes other hosts"] = function()
    expect.equal(hosts.forward({}, "go"), false)
  end,

  ["remote actions attach then execute on their owner and relay client actions"] = function(t)
    local calls, queued = {}, {}
    t.stub(rex, "server", function(label)
      expect.equal(label, "studio")
      return {
        call = function(method, args)
          calls[#calls + 1] = { method, args }
          if method == "session.attach" then
            return {}
          end
          return {
            result = { owner = "studio" },
            client_actions = { { action = "client.sidebar.toggle", args = {} } },
          }
        end,
      }
    end)
    t.stub(rex.client, "queue", function(action, args)
      queued[#queued + 1] = { action, args }
    end)
    local ctx = { server = "studio", session_id = "s", block_id = "b", origin = "key" }
    local args = { query = "https://github.com/example/demo/pull/7" }
    local sent, value = hosts.forward(ctx, "go_start_pr", args)
    expect.equal({ sent, value }, { true, { owner = "studio" } })
    expect.equal(calls, {
      { "session.attach", { session_id = "s" } },
      {
        "client.action",
        { action = "go_start_pr", args = args, session_id = "s", block_id = "b", origin = "key" },
      },
    })
    expect.equal(queued, { { "client.sidebar.toggle", {} } })
    expect.equal(ctx.server, "studio")
  end,

  ["an unavailable host never runs the action here"] = function(t)
    local calls = {}
    t.stub(rex, "server", function()
      return {
        call = function(method)
          calls[#calls + 1] = method
          return nil, "host unavailable"
        end,
      }
    end)
    expect.fails(function()
      hosts.forward({ server = "studio", session_id = "s" }, "go")
    end, "running go on studio: host unavailable")
    expect.equal(calls, { "session.attach" })
  end,

  ["a failed owner action never retries or falls back"] = function(t)
    local calls = {}
    t.stub(rex, "server", function()
      return {
        call = function(method)
          calls[#calls + 1] = method
          if method == "session.attach" then
            return {}
          end
          return nil, "repository is unavailable"
        end,
      }
    end)
    expect.fails(function()
      hosts.forward({ server = "studio", session_id = "s" }, "go_start_new", { id = "dir|/repo" })
    end, "repository is unavailable")
    expect.equal(calls, { "session.attach", "client.action" })
  end,

  ["script origin is translated to an allowed native RPC origin"] = function(t)
    t.stub(rex, "server", function()
      return {
        call = function(method, args)
          expect.equal(method, "client.action")
          expect.equal(args.origin, "api")
          return {}
        end,
      }
    end)
    expect.equal(hosts.forward({ server = "studio", origin = "script" }, "go_view"), true)
  end,

  ["canonical host identity and display name stay separate"] = function(t)
    t.stub(rex, "call", function(method)
      expect.equal(method, "server.status")
      return { host_id = "host-one" }
    end)
    t.stub(process, "run", function(argv, opts)
      expect.equal(argv, { "uname", "-n" })
      expect.equal(opts.timeout, 3)
      return { ok = true, stdout = "studio.example\n" }
    end)
    expect.equal(hosts.current(), { id = "host-one", label = "studio.example" })
  end,

  ["a missing native host identity fails before reading the hostname"] = function(t)
    t.stub(rex, "call", function()
      return {}
    end)
    t.stub(process, "run", function()
      error("must not run")
    end)
    expect.fails(hosts.current, "needs a state directory")
  end,

  ["hostname failures are errors rather than a different host"] = function(t)
    t.stub(rex, "call", function()
      return { host_id = "host-one" }
    end)
    t.stub(process, "run", function()
      return { ok = false, stderr = "cannot read hostname" }
    end)
    expect.fails(hosts.current, "cannot read hostname")
    t.stub(process, "run", function()
      return { ok = true, stdout = "\n" }
    end)
    expect.fails(hosts.current, "empty host name")
  end,
}
