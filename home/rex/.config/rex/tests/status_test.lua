local expect = require("tests.expect")
local status = require("dots.status")

return {
  ["only exact current-process runtime ids identify an agent"] = function()
    local id = "pi/0123456789ab7cde8fab0123456789ab/0123456789abcdef"
    local record = { state = "working", app = "pi", id = id, owner = { pid = 42 } }
    expect.equal(
      status.agent_owner({ record }, 42),
      "pi-01234567-89ab-7cde-8fab-0123456789ab-0123456789abcdef"
    )
    expect.equal(status.agent_owner({ record }, 43), nil)
    expect.equal(status.agent_owner({ record }, nil), nil)
    expect.equal(
      status.agent_owner(
        { { state = "working", app = "pi", id = "pi/bad/0123456789abcdef", owner = { pid = 42 } } },
        42
      ),
      nil
    )
    expect.equal(
      status.agent_owner({ { state = "working", id = id, owner = { pid = 42 } } }, 42),
      nil
    )
    expect.equal(
      status.agent_owner({
        record,
        {
          state = "working",
          app = "pi",
          id = "pi/0123456789ab7cde8fab0123456789ab/fedcba9876543210",
          owner = { pid = 42 },
        },
      }, 42),
      nil
    )
  end,

  ["a named Pi root wins equal-rank jobs but not more urgent records"] = function()
    local root = {
      state = "working",
      app = "pi",
      id = "pi/0123456789ab7cde8fab0123456789ab/0123456789abcdef",
      msg = "service cleanup",
    }
    local job = { state = "working", app = "pi", id = "job/j-1", title = "cargo test" }
    expect.equal(status.headline({ job, root }), root)
    job.state = "blocked"
    expect.equal(status.headline({ job, root }), job)
  end,

  ["the most urgent record heads a terminal"] = function()
    local records = {
      { state = "working", id = "build" },
      { state = "done", id = "test" },
      { state = "blocked", id = "deploy" },
      { state = "idle" },
    }
    expect.equal(status.headline(records).id, "deploy")
  end,

  ["unknown states are ignored and no records mean no headline"] = function()
    ---@diagnostic disable-next-line: assign-type-mismatch -- a newer state this code does not know
    expect.equal(status.headline({ { state = "sleeping" } }), nil)
    expect.equal(status.headline({}), nil)
  end,

  ["blocked, error, and done want the user"] = function()
    for _, state in ipairs({ "blocked", "error", "done" }) do
      expect.equal(status.needs_attention(state), true)
    end
    for _, state in ipairs({ "working", "idle" }) do
      expect.equal(status.needs_attention(state), false)
    end
  end,
}
