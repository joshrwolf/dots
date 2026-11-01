local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local go = require("dots.go")
local jumps = require("dots.jumps")
local process = require("dots.process")
local snapshot_module = require("dots.snapshot")
local start = require("dots.start")
local views = require("dots.views")

local snapshot = {
  home = "/home/me",
  sessions = {
    {
      id = "s1",
      label = "dots",
      blocks = {
        { id = "b1", window_id = "w1", tab = 1, cwd = "/home/me/dots" },
        {
          id = "b2",
          window_id = "w1",
          tab = 1,
          cwd = "/home/me/dots",
          status = {
            state = "working",
            app = "pi",
            msg = "editing",
            updated_at = "2026-10-20T10:00:00Z",
          },
        },
      },
    },
    {
      id = "s2",
      label = "api",
      blocks = {
        {
          id = "b3",
          window_id = "w2",
          tab = 1,
          cwd = "/src/api",
          status = { state = "blocked", effective_app = "claude-code", msg = "approve?" },
        },
      },
    },
  },
  worktrees = { "/home/me/dots", "/home/me/dots-feature" },
  dirs = { "/home/me/dots", "/home/me/dev/api" },
}

local function ids(view, group)
  local out = {}
  for _, row in ipairs(view.rows) do
    if row.group == group then
      out[#out + 1] = row.id
    end
  end
  return out
end

local function row(view, id)
  for _, candidate in ipairs(view.rows) do
    if candidate.id == id then
      return candidate
    end
  end
  error("no row " .. id)
end

local function texts(r)
  local out = {}
  for _, cell in ipairs(r.cells) do
    out[#out + 1] = cell.text
  end
  return out
end

-- A Rex with a session whose shell is in /tmp/work, beside the picker that
-- chose the row, and an app showing the session the user is in.
---@return tests.FakeRex
local function rex_at_work()
  local terminal = "com.superlogical.terminal"
  return {
    sessions = {
      {
        session_id = "s1",
        label = "work",
        view = {
          session_id = "s1",
          revision = 1,
          windows = {
            {
              window_id = "w1",
              layers = {
                { blocks = { { block_id = "shell", creator_name = terminal } } },
                { blocks = { { block_id = "picker", creator_name = terminal } } },
              },
            },
          },
        },
        terminals = {
          shell = { process = { foreground = { cwd = "/tmp/work" } } },
          picker = { process = { foreground = { cwd = "/home/me" } } },
        },
      },
    },
    clients = {
      { client_id = "other", info = { kind = "app" }, session_ids = { "s9" } },
      { client_id = "mine", info = { kind = "app" }, session_ids = { "s1" } },
    },
  }
end

local ctx = { session_id = "s1", block_id = "picker" }

---@param session_id string
---@param window_id? string
---@return tests.Call
local function shows(session_id, window_id)
  return {
    method = "client.perform",
    args = {
      client_id = "mine",
      action = "session.select",
      args = { session_id = session_id, window_id = window_id },
    },
  }
end

return {
  ["groups rows by what wants the user"] = function()
    local view = go.view(snapshot)
    expect.equal(ids(view, "attention"), { "block|s2|b3" })
    -- The api tab's one agent is in attention already.
    expect.equal(ids(view, "tabs"), { "tab|s1|w1" })
    local groups = {}
    for _, group in ipairs(view.groups) do
      groups[#groups + 1] = group.id
    end
    expect.equal(groups, { "attention", "tabs", "worktrees", "dirs" })
  end,

  ["places appear once, even where a tab is open"] = function()
    local view = go.view(snapshot)
    -- /home/me/dots is a repo with a tab open: listed as a directory, where
    -- new work starts, and not also as one of its worktrees.
    expect.equal(ids(view, "worktrees"), { "dir|/home/me/dots-feature" })
    expect.equal(ids(view, "dirs"), { "dir|/home/me/dots", "dir|/home/me/dev/api" })
  end,

  ["a tab shows the status of the agent at work in it"] = function()
    local tab = row(go.view(snapshot), "tab|s1|w1")
    expect.equal(texts(tab), { "◐", "dots", "pi", "editing" })
    expect.equal(tab.cells[1].role, "yellow")
    expect.equal(tab.preview, { "rex", "capture", "-s", "s1", "-w", "w1", "--format=vt", "--trim" })
  end,

  ["an attention row leads with its session and focuses its terminal"] = function()
    local agent = row(go.view(snapshot), "block|s2|b3")
    -- Its directory only repeats the session, so its agent says what it is.
    expect.equal(texts(agent), { "●", "api", "claude-code", "approve?" })
    expect.equal(
      agent.preview,
      { "rex", "capture", "-s", "s2", "-b", "b3", "--format=vt", "--trim" }
    )
  end,

  ["a tab beside an agent needing attention shows the next agent at work"] = function()
    local session = {
      id = "s",
      label = "work",
      blocks = {
        { id = "done", window_id = "w", tab = 1, status = { state = "done", app = "pi" } },
        { id = "busy", window_id = "w", tab = 1, status = { state = "working", app = "pi" } },
        { id = "idle", window_id = "w", tab = 1, status = { state = "idle", app = "pi" } },
      },
    }
    local view = go.view({ sessions = { session }, worktrees = {}, dirs = {} })
    expect.equal(ids(view, "attention"), { "block|s|done" })
    expect.equal(ids(view, "tabs"), { "tab|s|w" })
    expect.equal(texts(row(view, "tab|s|w")), { "◐", "work", "pi", "" })
  end,

  ["a tab without an agent shows its program, never the shell"] = function()
    local session = {
      id = "s",
      label = "work",
      blocks = {
        { id = "a", window_id = "w1", tab = 1, cwd = "/src/api", program = "nvim", focused = true },
        { id = "b", window_id = "w2", tab = 2, cwd = "/src/api" },
      },
    }
    local view = go.view({ sessions = { session }, worktrees = {}, dirs = {} })
    expect.equal(texts(view.rows[1]), { "", "work", "api · tab 1", "nvim" })
    expect.equal(texts(view.rows[2]), { "", "work", "api · tab 2", "" })
  end,

  ["places line up with the session column and show their worktree"] = function()
    local snap = {
      home = "/home/me",
      sessions = {},
      worktrees = { "/home/me/.wt/home/me/src/dots/fix-login" },
      dirs = { "/home/me/dev/notes" },
    }
    local view = go.view(snap)
    expect.equal(texts(view.rows[1]), { "", "dots@fix-login", "~/.wt/home/me/src/dots/fix-login" })
    expect.equal(texts(view.rows[2]), { "", "notes", "~/dev/notes" })
  end,

  ["every row belongs to a declared group"] = function()
    local view = go.view(snapshot)
    local declared = {}
    for _, group in ipairs(view.groups) do
      declared[group.id] = #group.columns
    end
    for _, r in ipairs(view.rows) do
      expect.equal(#r.cells, declared[r.group])
    end
  end,

  ["row ids round-trip"] = function()
    expect.equal(go.parse_id("tab|s1|w1"), { kind = "tab", session = "s1", window = "w1" })
    expect.equal(go.parse_id("block|s1|b2"), { kind = "block", session = "s1", block = "b2" })
    expect.equal(go.parse_id("dir|/a|b"), { kind = "dir", path = "/a|b" })
    expect.equal(go.parse_id("nonsense"), nil)
  end,

  ["a tab is described by its most urgent agent, else its focused terminal"] = function()
    local session = {
      id = "s",
      label = "s",
      blocks = {
        { id = "a", window_id = "w1", tab = 1, program = "zsh" },
        { id = "b", window_id = "w1", tab = 1, program = "nvim", focused = true },
        { id = "c", window_id = "w2", tab = 2, status = { state = "idle" } },
        { id = "d", window_id = "w2", tab = 2, status = { state = "working" } },
        { id = "e", window_id = "w2", tab = 2, focused = true },
        { id = "f", window_id = "w3", tab = 3, status = { state = "blocked" } },
      },
    }
    local tabs = go.tabs(session)
    local described = {}
    for _, tab in ipairs(tabs) do
      described[#described + 1] = tab.block.id .. (tab.attended and " attended" or "")
    end
    expect.equal(described, { "b", "d", "f attended" })
  end,

  ["go_to a tab shows its session at that window in the user's app"] = function(t)
    local fake = fake_rex.install(t, rex_at_work())
    go.go_to(ctx, { id = "tab|s2|w7" })
    expect.equal(fake.only("client.perform"), { shows("s2", "w7") })
  end,

  ["go_to a terminal focuses it, then shows its session at its window"] = function(t)
    local world = rex_at_work()
    world.sessions[#world.sessions + 1] = {
      session_id = "s2",
      label = "other",
      view = {
        session_id = "s2",
        revision = 1,
        windows = {
          { window_id = "w5", layers = { { kind = "tiled", blocks = { { block_id = "b5" } } } } },
        },
      },
    }
    local fake = fake_rex.install(t, world)
    go.go_to(ctx, { id = "block|s2|b5" })
    expect.equal(fake.only("session.attach", "session.focus_block", "client.perform"), {
      -- Reading where the jump starts.
      { method = "session.attach", args = { session_id = "s1" } },
      { method = "session.attach", args = { session_id = "s2" } },
      { method = "session.focus_block", args = { session_id = "s2", block_id = "b5" } },
      -- Finding the terminal's window, so the app shows that one.
      { method = "session.attach", args = { session_id = "s2" } },
      shows("s2", "w5"),
    })
  end,

  ["go_to a terminal that cannot be focused shows nothing"] = function(t)
    local fake = fake_rex.install(t, rex_at_work())
    fake.errors = { ["session.focus_block"] = "block not found" }
    expect.fails(function()
      go.go_to(ctx, { id = "block|s2|b5" })
    end, "focusing the terminal: block not found")
    expect.equal(fake.only("client.perform"), {})
  end,

  ["go_to a place with a session reuses it, /private or not"] = function(t)
    local fake = fake_rex.install(t, rex_at_work())
    go.go_to(ctx, { id = "dir|/private/tmp/work" })
    expect.equal(fake.only("session.create", "client.perform"), { shows("s1") })
  end,

  ["go_to a new place creates a session named for it, then shows it"] = function(t)
    local fake = fake_rex.install(t, rex_at_work())
    go.go_to(ctx, { id = "dir|/src/new-thing/" })
    expect.equal(fake.only("session.create", "client.perform"), {
      {
        method = "session.create",
        args = {
          label = "new-thing",
          initial_windows = {
            {
              layout = rex.layout.block({
                flavor = "com.superlogical.terminal.shell",
                options = { cwd = "/src/new-thing/" },
              }),
            },
          },
        },
      },
      shows("created"),
    })
  end,

  ["go_to ignores the picker's own directory"] = function(t)
    local fake = fake_rex.install(t, rex_at_work())
    go.go_to(ctx, { id = "dir|/home/me" })
    expect.equal(#fake.only("session.create"), 1)
    expect.equal(fake.only("session.create")[1].args.label, "me")
    expect.equal(fake.only("client.perform"), { shows("created") })
  end,

  ["go_to refuses an id it cannot read"] = function(t)
    local fake = fake_rex.install(t, rex_at_work())
    expect.fails(function()
      go.go_to(ctx, { id = "nonsense" })
    end, "go_to cannot read row id nonsense")
    expect.equal(fake.calls, {})
  end,

  ["recent tabs lead the tabs group"] = function()
    local session = {
      id = "s3",
      label = "notes",
      blocks = { { id = "b4", window_id = "w3", tab = 1 } },
    }
    local snap = { sessions = { snapshot.sessions[1], session }, worktrees = {}, dirs = {} }
    local view = go.view(snap, { { session_id = "s3", window_id = "w3" } })
    expect.equal(ids(view, "tabs"), { "tab|s3|w3", "tab|s1|w1" })
  end,

  ["go_to records the tab it leaves and the one it reaches"] = function(t)
    t.stub(jumps, "recent", {})
    fake_rex.install(t, rex_at_work())
    go.go_to(ctx, { id = "tab|s2|w7" })
    expect.equal(jumps.recent, {
      { session_id = "s2", window_id = "w7" },
      { session_id = "s1", window_id = "w1" },
    })
  end,

  ["go_to a place records the session's active window"] = function(t)
    t.stub(jumps, "recent", {})
    local fake = rex_at_work()
    fake.sessions[1].view.windows[2] = { window_id = "w2", layers = {} }
    fake.sessions[1].view.active_window_id = "w2"
    fake_rex.install(t, fake)
    go.go_to({ session_id = "s1", block_id = "picker" }, { id = "dir|/tmp/work" })
    expect.equal(jumps.recent, {
      { session_id = "s1", window_id = "w2" },
      { session_id = "s1", window_id = "w1" },
    })
  end,
  ["start keys say what they take, and the new key takes a row instead"] = function()
    local keys = {}
    for _, shortcut in ipairs(go.view(snapshot).shortcuts) do
      keys[shortcut.action] = { shortcut.key, shortcut.label, shortcut.query, shortcut.hint }
    end
    expect.equal(keys, {
      go_start_pr = { "r", "review PR", true, "URL" },
      go_start_issue = { "t", "start issue", true, "ID or URL" },
      go_start_new = { "o", "new session" },
    })
  end,

  ["a row stands for its directory"] = function()
    expect.equal(go.place(snapshot, "tab|s1|w1"), "/home/me/dots")
    expect.equal(go.place(snapshot, "block|s2|b3"), "/src/api")
    expect.equal(go.place(snapshot, "dir|/tmp"), "/tmp")
    expect.equal(go.place(snapshot, "tab|s9|w1"), nil)
    expect.equal(go.place(snapshot, "nonsense"), nil)
  end,

  ["Go opens where the user was, so a start can use its repo"] = function(t)
    t.stub(process, "which", function(name)
      return "/bin/" .. name
    end)
    t.stub(views, "open_views", {})
    local fake = fake_rex.install(t, rex_at_work())
    local actions = fake_rex.registered(t, go)
    actions.go.run({ session_id = "s1", block_id = "shell" }, {})
    local opened = fake.only("session.new_layer")[1].args
    expect.equal(opened.window_id, "w1")
    expect.equal(
      opened.layout,
      rex.layout.block({
        flavor = "com.superlogical.terminal.shell",
        label = "go",
        options = {
          command = { "/bin/dots-pick", "go_view", "--dismiss", "views_dismiss" },
          cwd = "/tmp/work",
        },
      })
    )
  end,

  ["a start from a malformed line says why and creates nothing"] = function(t)
    local fake = fake_rex.install(t, rex_at_work())
    expect.fails(function()
      go.start(ctx, "issue", { query = "abc 12" })
    end, "abc 12 is not a Linear issue; expected an ID or URL like ABC-123")
    expect.equal(fake.only("session.create", "exec"), {})
  end,

  ["a new session starts in the repo of the selected row"] = function(t)
    local function start_layout(repo)
      return rex.layout.block({
        flavor = "com.superlogical.terminal.shell",
        options = {
          command = start.argv("twilight-prairie", true, "shell"),
          cwd = repo,
          exit = { quick_exit_threshold_ms = 300000 },
        },
      })
    end
    local fake = rex_at_work()
    fake.commands = {
      ["git -C /home/me/dots rev-parse --path-format=absolute --git-common-dir"] = {
        stdout = "/src/dots/.git\n",
      },
      ["git -C /tmp rev-parse --path-format=absolute --git-common-dir"] = {
        stdout = "/src/tmp/.git\n",
      },
      ["git -C /src/api rev-parse --path-format=absolute --git-common-dir"] = {
        stdout = "/src/api/.git\n",
      },
      ["git -C /home/me rev-parse --path-format=absolute --git-common-dir"] = { ok = false },
    }
    for _, repo in ipairs({ "/src/dots", "/src/tmp", "/src/api" }) do
      fake.commands["git -C " .. repo .. " for-each-ref --format=%(refname) refs/heads/"] =
        { stdout = "refs/heads/main\n" }
      fake.commands["git -C " .. repo .. " worktree list --porcelain -z"] = { stdout = "" }
    end
    t.stub(jumps, "recent", {})
    local installed = fake_rex.install(t, fake)
    t.stub(snapshot_module, "collect", function()
      return snapshot
    end)
    for _, case in ipairs({
      { "tab|s1|w1", "dots" },
      { "block|s2|b3", "api" },
      { "dir|/tmp", "tmp" },
    }) do
      local draws = { 46, 34 }
      go.start_new(ctx, { id = case[1] }, function()
        return table.remove(draws, 1)
      end)
      local created = installed.only("session.create")
      local args = created[#created].args
      expect.equal({ case[1], args.label, args.initial_windows[1].layout }, {
        case[1],
        case[2] .. ":twilight-prairie",
        start_layout("/src/" .. case[2]),
      })
    end
  end,

  ["a new session without a selected repo says to pick one and creates nothing"] = function(t)
    local fake = rex_at_work()
    fake.commands = {
      ["git -C /tmp rev-parse --path-format=absolute --git-common-dir"] = { ok = false },
    }
    local installed = fake_rex.install(t, fake)
    t.stub(snapshot_module, "collect", function()
      return snapshot
    end)
    for _, args in ipairs({ {}, { id = "dir|/tmp" } }) do
      expect.fails(function()
        go.start_new(ctx, args, math.random)
      end, "select a repo first: type / and pick one")
    end
    expect.equal(installed.only("session.create"), {})
  end,
}
