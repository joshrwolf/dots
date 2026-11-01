local attention = require("dots.attention")
local expect = require("tests.expect")

local function block(id, state, at)
  return { id = id, window_id = "w", tab = 1, status = { state = state, app = id, updated_at = at } }
end

local snap = {
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

local function ids(view)
  local out = {}
  for _, row in ipairs(view.rows) do
    out[#out + 1] = row.group .. " " .. row.id
  end
  return out
end

return {
  ["lists agents wanting the user by urgency, then working ones; never idle or plain shells"] = function()
    expect.equal(ids(attention.view(snap)), {
      "attention block|s2|blocked",
      "attention block|s2|done-new",
      "attention block|s1|done-old",
      "working block|s1|working",
    })
  end,

  ["rows open through go_to and preview their terminal"] = function()
    local view = attention.view(snap)
    expect.equal(view.accept, "go_to")
    expect.equal(
      view.rows[1].preview,
      { "rex", "capture", "-s", "s2", "-b", "blocked", "--format=vt", "--trim" }
    )
  end,

  ["nothing wanting attention is an empty view"] = function()
    local quiet = {
      sessions = { { id = "s", label = "s", blocks = { block("i", "idle") } } },
      worktrees = {},
      dirs = {},
    }
    expect.equal(attention.view(quiet).rows, {})
  end,
}
