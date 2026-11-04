local expect = require("tests.expect")
local process = require("dots.process")

local function environment(t)
  t.stub(process, "environment", function()
    return { PATH = "/tools:/usr/bin", ANTIDOTE_HOME = "/packages/antidote" }
  end)
end

return {
  ["reads the machine environment from HOME instead of a repository"] = function(t)
    t.stub(os, "getenv", function(name)
      return ({ HOME = "/home/me", PATH = "/usr/bin", MISE_BIN = "/home/me/.local/bin/mise" })[name]
    end)
    local calls = {}
    t.stub(rex, "exec", function(argv, opts)
      calls[#calls + 1] = { argv = argv, opts = opts }
      return {
        ok = true,
        stdout = '{"PATH":"/tools:/usr/bin","ANTIDOTE_HOME":"/packages/antidote"}',
      }
    end)
    expect.equal(
      process.environment(),
      { PATH = "/tools:/usr/bin", ANTIDOTE_HOME = "/packages/antidote" }
    )
    expect.equal(calls[1].argv, { "/home/me/.local/bin/mise", "env", "--json" })
    expect.equal(calls[1].opts.cwd, "/home/me")
    expect.equal(calls[1].opts.env.PATH, "/home/me/.local/bin:/usr/bin")
  end,

  ["keeps explicit environment overrides and the command directory"] = function(t)
    environment(t)
    local opts = { cwd = "/src", env = { PATH = "/x", CI = "1" } }
    expect.equal(process.with_path(opts), {
      cwd = "/src",
      env = { PATH = "/x", CI = "1", ANTIDOTE_HOME = "/packages/antidote" },
    })
    expect.equal(opts, { cwd = "/src", env = { PATH = "/x", CI = "1" } })
  end,

  ["reuses an operation environment without leaking command overrides"] = function(t)
    t.stub(process, "environment", function()
      error("must use the supplied environment")
    end)
    local shared = { PATH = "/tools:/usr/bin", CI = "0" }
    local calls = {}
    t.stub(rex, "exec", function(argv, opts)
      calls[#calls + 1] = { argv = argv, opts = opts }
      return { ok = true, stdout = "ready" }
    end)
    local first =
      assert(process.run({ "git", "status" }, { cwd = "/src", env = { CI = "1" } }, shared))
    local second = assert(process.run({ "zoxide", "query", "--list" }, { timeout = 5 }, shared))
    expect.equal(first.ok, true)
    expect.equal(second.stdout, "ready")
    expect.equal(calls[1].opts.env, { PATH = "/tools:/usr/bin", CI = "1" })
    expect.equal(calls[1].opts.cwd, "/src")
    expect.equal(calls[2].opts.env, shared)
    expect.equal(calls[2].opts.timeout, 5)
    expect.equal(shared, { PATH = "/tools:/usr/bin", CI = "0" })
    expect.equal(calls[2].opts.env == shared, false)
  end,

  ["finds commands on the selected path"] = function(t)
    environment(t)
    expect.equal(
      process.which("tool", function(path)
        return path == "/tools/tool"
      end),
      "/tools/tool"
    )
    expect.equal(
      process.which("missing", function()
        return false
      end),
      nil
    )
    expect.equal(process.which("/abs/tool"), "/abs/tool")
  end,

  ["environment failure prevents command dispatch"] = function(t)
    local calls = 0
    t.stub(rex, "exec", function()
      calls = calls + 1
      return nil, "mise is missing"
    end)
    local result, err = process.run({ "git", "status" })
    expect.equal(result, nil)
    expect.equal(err, "resolving the machine environment: mise is missing")
    expect.equal(calls, 1)
  end,

  ["bad environment output is a failure rather than an inherited PATH"] = function(t)
    for _, stdout in ipairs({ "invalid", '{"PATH":false}', '{"PATH":"/tools","INVALID":7}' }) do
      t.stub(rex, "exec", function()
        return { ok = true, stdout = stdout }
      end)
      local result, err = process.environment()
      expect.equal(result, nil)
      expect.equal(type(err), "string")
    end
  end,

  ["mise command failure reaches the caller"] = function(t)
    t.stub(rex, "exec", function()
      return { ok = false, stderr = "config is not trusted" }
    end)
    local result, err = process.environment()
    expect.equal(result, nil)
    expect.equal(err, "resolving the machine environment: config is not trusted")
  end,
}
