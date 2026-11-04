local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local snapshot = require("dots.snapshot")
local process = require("dots.process")

local TERMINAL = "com.superlogical.terminal"
local SHELL = "com.superlogical.terminal.shell"

---@return tests.FakeRex|{ sessions: tests.FakeSession[] }
local function rex_with_two_sessions()
  return {
    sessions = {
      {
        session_id = "s1",
        label = "dots",
        view = {
          session_id = "s1",
          revision = 1,
          active_window_id = "w2",
          windows = {
            {
              window_id = "window:0001-1234abcd",
              label = "Window 1234abcd",
              focused_block_id = "b1",
              layers = {
                {
                  blocks = {
                    { block_id = "b1", creator_name = TERMINAL, flavor = SHELL, label = "shell" },
                  },
                },
              },
            },
            {
              window_id = "w2",
              label = "review",
              focused_block_id = "b2",
              layers = {
                {
                  blocks = {
                    { block_id = "b2", creator_name = TERMINAL, flavor = SHELL, label = "worker" },
                    { block_id = "picker", creator_name = TERMINAL },
                  },
                },
                { blocks = { { block_id = "other", creator_name = "com.example.widget" } } },
              },
            },
          },
        },
        terminals = {
          b1 = { process = { foreground = { cwd = "/src/dots", name = "zsh", argv0 = "-zsh" } } },
          b2 = {
            process = { foreground = { cwd = "/src/dots/sub", name = "node", argv0 = "pi" } },
            records = {
              { state = "working", app = "pi" },
              { state = "blocked", app = "pi", id = "ask" },
            },
          },
          picker = { process = { foreground = { cwd = "/tmp" } } },
        },
      },
      {
        session_id = "s2",
        label = "api",
        view = { session_id = "s2", revision = 1, windows = {} },
        terminals = {},
      },
    },
  }
end

local snap = {
  sessions = {
    { id = "s1", label = "a", blocks = { { id = "b1", window_id = "w", cwd = "/src/a" } } },
    { id = "s2", label = "b", blocks = { { id = "b2", window_id = "w", cwd = "/src/b" } } },
  },
  worktrees = {},
  dirs = {},
}

return {
  ["directories are primary checkouts under ~/dev"] = function()
    local checkouts = {
      ["/home/me/dev/work/api/.git/HEAD"] = true,
      ["/home/me/dev/web/.git/HEAD"] = true,
      ["/tmp/scratch/.git/HEAD"] = true,
    }
    local dirs = {
      "/home/me/dev/work/api",
      "/home/me/dev/work", -- only groups repos
      "/home/me/dev/work/api/src", -- inside a repo
      "/tmp/scratch", -- outside ~/dev
      "/home/me/dev/web",
    }
    local function exists(path)
      return checkouts[path] == true
    end
    expect.equal(
      snapshot.repos(dirs, "/home/me", exists),
      { "/home/me/dev/work/api", "/home/me/dev/web" }
    )
    expect.equal(snapshot.repos(dirs, nil, exists), {})
  end,

  ["reads worktree paths from NUL-separated porcelain"] = function()
    local porcelain = table.concat({
      "worktree /src/a",
      "HEAD abc",
      "branch refs/heads/main",
      "",
      "worktree /src/a with\ttab and\nnewline",
      "HEAD def",
      "detached",
      "",
    }, "\0") .. "\0"
    expect.equal(snapshot.worktree_paths(porcelain), { "/src/a", "/src/a with\ttab and\nnewline" })
  end,

  ["finds the session with a terminal in a place"] = function()
    expect.equal(snapshot.session_at(snap, "/src/b"), "s2")
    expect.equal(snapshot.session_at(snap, "/src/c"), nil)
  end,

  ["macOS /private paths match their short form"] = function()
    local tmp = { sessions = { { id = "t", blocks = { { id = "b", cwd = "/tmp/work" } } } } }
    expect.equal(snapshot.session_at(tmp, "/private/tmp/work"), "t")
    expect.equal(snapshot.session_at(tmp, "/private/tmpwork"), nil)
    local long = { sessions = { { id = "v", blocks = { { id = "b", cwd = "/private/var/x" } } } } }
    expect.equal(snapshot.session_at(long, "/var/x"), "v")
  end,

  ["places also depend on directory and process changes"] = function()
    local base = snapshot.events()
    local places = snapshot.events({ places = true })
    expect.equal(#places, #base + 2)
    for _, event in ipairs({ "session_view_changed", "terminal.program_status_changed" }) do
      local found = false
      for _, name in ipairs(base) do
        found = found or name == event
      end
      expect.equal(found, true)
    end
  end,

  ["collects every terminal with its directory and most urgent status"] = function(t)
    fake_rex.install(t, rex_with_two_sessions())
    local result = snapshot.collect({ exclude = "picker" })
    local s1, s2 = result.sessions[1], result.sessions[2]
    expect.equal({ s1.id, s1.label, s2.id, s2.label }, { "s1", "dots", "s2", "api" })
    expect.equal(#s1.blocks, 2) -- the picker and the non-terminal are left out
    expect.equal({ s1.blocks[1].cwd, s1.blocks[2].cwd }, { "/src/dots", "/src/dots/sub" })
    expect.equal({ s1.blocks[1].focused, s1.blocks[2].focused }, { true, true }) -- each tab's focused terminal
    expect.equal(s1.blocks[2].status.state, "blocked")
    expect.equal(s1.blocks[1].status, nil)
    expect.equal(s2.blocks, {})
  end,

  ["places resolve one environment per collection, fresh on refresh"] = function(t)
    fake_rex.install(t, rex_with_two_sessions())
    local resolutions, calls = 0, {}
    t.stub(process, "environment", function()
      resolutions = resolutions + 1
      return { PATH = "/tools/" .. resolutions }
    end)
    t.stub(process, "run", function(argv, opts, environment)
      calls[#calls + 1] = { command = argv[1], path = environment.PATH }
      expect.equal(opts.timeout, 5)
      if argv[1] == "git" then
        return { ok = true, stdout = "worktree /src/dots\0\0worktree /trees/dots/topic\0\0" }
      end
      expect.equal(argv, { "zoxide", "query", "--list" })
      return { ok = true, stdout = "" }
    end)
    for n = 1, 2 do
      local result = snapshot.collect({ places = true, exclude = "picker" })
      expect.equal(result.worktrees, { "/src/dots", "/trees/dots/topic" })
      expect.equal(resolutions, n)
      expect.equal(#calls, n * 3)
      for i = (n - 1) * 3 + 1, n * 3 do
        expect.equal(calls[i].path, "/tools/" .. n)
      end
    end
  end,

  ["one listing covers every known linked root"] = function(t)
    local world = rex_with_two_sessions()
    world.sessions[1].terminals.b1.process.foreground.cwd = "/src/dots"
    world.sessions[1].terminals.b2.process.foreground.cwd = "/trees/dots/topic"
    fake_rex.install(t, world)
    local queries = {}
    t.stub(process, "run", function(argv)
      if argv[1] == "zoxide" then
        return { ok = true, stdout = "" }
      end
      queries[#queries + 1] = argv[3]
      return {
        ok = true,
        stdout = "worktree /src/dots\0\0worktree /trees/dots/topic\0\0",
      }
    end)
    local result = snapshot.collect({ places = true, exclude = "picker" })
    expect.equal(queries, { "/src/dots" })
    expect.equal(result.worktrees, { "/src/dots", "/trees/dots/topic" })
  end,

  ["a nested repository is queried even inside an already listed root"] = function(t)
    fake_rex.install(t, rex_with_two_sessions())
    local queries = {}
    t.stub(process, "run", function(argv)
      if argv[1] == "zoxide" then
        return { ok = true, stdout = "" }
      end
      queries[#queries + 1] = argv[3]
      return { ok = true, stdout = "worktree " .. argv[3] .. "\0\0" }
    end)
    expect.equal(snapshot.collect({ places = true, exclude = "picker" }).worktrees, {
      "/src/dots",
      "/src/dots/sub",
    })
    expect.equal(queries, { "/src/dots", "/src/dots/sub" })
  end,

  ["environment failure is visible, but session-only collection still works"] = function(t)
    local fake = fake_rex.install(t, rex_with_two_sessions())
    t.stub(process, "environment", function()
      return nil, "machine environment unavailable"
    end)
    expect.equal(#snapshot.collect({ exclude = "picker" }).sessions, 2)
    expect.fails(function()
      snapshot.collect({ places = true, exclude = "picker" })
    end, "reading places: machine environment unavailable")
    expect.equal(fake.only("exec"), {})
  end,

  ["keeps available places when one repository cannot be read"] = function(t)
    local fake = rex_with_two_sessions()
    fake.commands = {
      ["git -C /src/dots worktree list --porcelain -z"] = { ok = false },
      ["git -C /src/dots/sub worktree list --porcelain -z"] = {
        stdout = "worktree /src/dots\0\0worktree /trees/dots/topic\0\0",
      },
      ["zoxide query --list"] = { stdout = "" },
    }
    fake_rex.install(t, fake)
    expect.equal(snapshot.collect({ places = true, exclude = "picker" }).worktrees, {
      "/src/dots",
      "/trees/dots/topic",
    })
  end,

  ["keeps the labels someone set and the program as invoked"] = function(t)
    fake_rex.install(t, rex_with_two_sessions())
    local blocks = snapshot.collect({ exclude = "picker" }).sessions[1].blocks
    local function read(block)
      return { block.tab, block.tab_label or "-", block.label or "-", block.program or "-" }
    end
    -- Rex's own labels name the window by id and the block by flavor; a
    -- login shell at its prompt runs no program.
    expect.equal(read(blocks[1]), { 1, "-", "-", "-" })
    expect.equal(read(blocks[2]), { 2, "review", "worker", "pi" })
  end,

  ["names a worktrunk worktree by repo and branch"] = function()
    local tree = "/home/me/.wt/home/me/src/dots/fix-login"
    local s = { home = "/home/me", worktrees = { "/home/me/src/dots", tree } }
    expect.equal(snapshot.name(tree, s), "dots@fix-login")
    expect.equal(snapshot.name(tree .. "/src/pick", s), "dots@fix-login/src/pick")
    expect.equal(snapshot.location(tree, s), "dots@fix-login")
    expect.equal(snapshot.name("/home/me/src/dots", s), "dots")
    expect.equal(snapshot.location("/home/me/src/dots", s), "~/src/dots")
    expect.equal(snapshot.location("/srv/data", s), "/srv/data")
    -- Without a worktree list, the path is taken to be a worktree.
    expect.equal(snapshot.name(tree, { home = "/home/me" }), "dots@fix-login")
    expect.equal(snapshot.name("/home/me/.wt/stray", s), "stray")
  end,

  ["attaches to each session before reading it"] = function(t)
    local fake = fake_rex.install(t, rex_with_two_sessions())
    snapshot.collect({})
    local order = {}
    for _, call in ipairs(fake.only("session.attach", "session.view")) do
      order[#order + 1] = call.method .. " " .. call.args.session_id
    end
    expect.equal(order, {
      "session.attach s1",
      "session.view s1",
      "session.attach s2",
      "session.view s2",
    })
  end,

  ["a session that cannot be read is still listed, with no terminals"] = function(t)
    local fake = rex_with_two_sessions()
    fake.sessions[2].view = nil
    fake_rex.install(t, fake)
    local result = snapshot.collect({})
    expect.equal(result.sessions[2].blocks, {})
  end,
}
