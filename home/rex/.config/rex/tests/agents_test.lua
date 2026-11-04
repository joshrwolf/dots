local agents = require("dots.agents")
local expect = require("tests.expect")

local HOME = "/home/me"
local WORKTREE = "/home/me/.wt/home/me/src/dots/fix-login"

---@param blocks dots.Block[]
---@param label? string
---@return dots.Snapshot
local function snap(blocks, label)
  return {
    home = HOME,
    sessions = { { id = "s", label = label or "work", blocks = blocks } },
    worktrees = {},
    dirs = {},
  }
end

---The what and detail cells of a tab row for each block, in order.
---@param s dots.Snapshot
---@return string[][]
local function read(s)
  local items = {}
  for _, block in ipairs(s.sessions[1].blocks) do
    items[#items + 1] = {
      group = "tabs",
      entry = { session = s.sessions[1], block = block, tab = true },
    }
  end
  local out = {}
  for _, row in ipairs(agents.rows(items, s)) do
    out[#out + 1] = { row.cells[3].text, row.cells[4].text }
  end
  return out
end

local pi = { state = "working", app = "pi", msg = "1 worker" }

return {
  ["a row names its terminal by label, then title, place, app, and tab"] = function()
    local s = snap({
      {
        id = "a",
        window_id = "w1",
        tab = 1,
        tab_label = "review",
        label = "worker",
        cwd = WORKTREE,
      },
      { id = "b", window_id = "w2", tab = 2, label = "worker", cwd = WORKTREE },
      {
        id = "c",
        window_id = "w3",
        tab = 3,
        cwd = WORKTREE,
        status = { state = "done", title = "Refactor" },
      },
      { id = "d", window_id = "w4", tab = 4, cwd = WORKTREE, status = pi },
      { id = "e", window_id = "w5", tab = 5, cwd = "/src/work", status = pi },
      { id = "f", window_id = "w6", tab = 6, cwd = "/src/work", program = "node" },
    })
    expect.equal(read(s), {
      { "review", "dots@fix-login" },
      { "worker", "dots@fix-login" },
      { "Refactor", "dots@fix-login" },
      { "dots@fix-login", "1 worker" },
      -- `/src/work` only repeats the session's label.
      { "pi", "1 worker" },
      { "tab 6", "node" },
    })
  end,

  ["rows of a session that read alike are told apart by tab"] = function()
    local s = snap({
      { id = "a", window_id = "w1", tab = 1, cwd = "/src/dots", status = pi },
      {
        id = "b",
        window_id = "w2",
        tab = 2,
        cwd = "/src/dots",
        status = { state = "done", app = "pi" },
        title = "π - dots",
      },
      { id = "c", window_id = "w3", tab = 3, cwd = "/src/api" },
    }, "agents")
    expect.equal(read(s), {
      { "dots · tab 1", "1 worker" },
      { "dots · tab 2", "π" },
      { "api", "" },
    })
  end,

  ["rows in different sessions may read alike"] = function()
    local one = { id = "a", window_id = "w", tab = 1, cwd = "/src/project", status = pi }
    local two = { id = "b", window_id = "w", tab = 1, cwd = "/src/project", status = pi }
    local s = {
      home = HOME,
      sessions = {
        { id = "s1", label = "harness", blocks = { one } },
        { id = "s2", label = "review", blocks = { two } },
      },
      worktrees = {},
      dirs = {},
    }
    local rows = agents.rows({
      { group = "tabs", entry = { session = s.sessions[1], block = one, tab = true } },
      { group = "tabs", entry = { session = s.sessions[2], block = two, tab = true } },
    }, s)
    expect.equal({ rows[1].cells[2].text, rows[1].cells[3].text }, { "harness", "project" })
    expect.equal({ rows[2].cells[2].text, rows[2].cells[3].text }, { "review", "project" })
  end,

  ["a worktree named like its session is not repeated"] = function()
    local s = snap({
      { id = "a", window_id = "w", tab = 1, tab_label = "build", cwd = WORKTREE },
    }, "dots:FIX-LOGIN")
    expect.equal(read(s), { { "build", "" } })
  end,

  ["a shell command reads as what it runs"] = function()
    expect.equal(
      agents.command("cd /home/me/.wt/x/dots/fix && node --no-warnings src/superfans/src/cli.ts"),
      "node --no-warnings cli.ts"
    )
    expect.equal(agents.command("cd a; cd b && FAST=1 X= cargo  build"), "cargo build")
    expect.equal(agents.command("curl https://example.com/a/b"), "curl https://example.com/a/b")
    expect.equal(agents.command("cd /tmp &&"), "cd tmp &&")
  end,

  ["only a shell's command is shortened"] = function()
    local command = { state = "working", app = "cd", msg = "cd /src/x && make all" }
    local said = { state = "working", app = "pi", msg = "editing src/a/b.rs" }
    expect.equal(agents.message(command), "make all")
    expect.equal(agents.message(said), "editing src/a/b.rs")
    expect.equal(agents.message({ state = "done", app = "pi" }, false, "π - dots"), "π")
    expect.equal(agents.message({ state = "done", app = "pi" }), "pi")
    expect.equal(agents.mark("✳ Fix login"), "✳")
    expect.equal(agents.mark("vim main.rs"), nil)
    expect.equal(agents.mark("π"), nil)
    expect.equal(agents.message({ state = "done", app = "pi" }, true), "")
    expect.equal(agents.message({ state = "idle" }), "")
  end,

  ["agents wanting the user come by urgency, then recency; never working, idle, or plain shells"] = function()
    local function block(id, state, at)
      return { id = id, window_id = "w", tab = 1, status = { state = state, updated_at = at } }
    end
    local s = {
      sessions = {
        {
          id = "s1",
          label = "dots",
          blocks = {
            block("idle", "idle", "1"),
            block("done-old", "done", "1"),
            block("working", "working", "2"),
            { id = "shell", window_id = "w", tab = 1 },
          },
        },
        {
          id = "s2",
          label = "api",
          blocks = { block("blocked", "blocked", "1"), block("done-new", "done", "3") },
        },
      },
      worktrees = {},
      dirs = {},
    }
    local ids = {}
    for _, entry in ipairs(agents.wanting(s)) do
      ids[#ids + 1] = entry.session.id .. "/" .. entry.block.id
    end
    expect.equal(ids, { "s2/blocked", "s2/done-new", "s1/done-old" })
  end,

  ["a row's status is its first column"] = function()
    local s = snap({
      { id = "a", window_id = "w", tab = 1, status = { state = "blocked", app = "pi" } },
      { id = "b", window_id = "w2", tab = 2 },
    })
    local rows = agents.rows({
      { group = "attention", entry = { session = s.sessions[1], block = s.sessions[1].blocks[1] } },
      {
        group = "tabs",
        entry = { session = s.sessions[1], block = s.sessions[1].blocks[2], tab = true },
      },
    }, s)
    expect.equal(
      { rows[1].id, rows[1].cells[1].text, rows[1].cells[1].role },
      { "block|s|a", "●", "red" }
    )
    expect.equal({ rows[2].id, rows[2].cells[1].text }, { "tab|s|w2", "" })
  end,
}
