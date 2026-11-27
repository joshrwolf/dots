local expect = require("tests.expect")
local status = require("dots.status")

return {
  ["the root record wins equal-rank children but not more urgent ones"] = function()
    local root = { state = "working", app = "pi", msg = "service cleanup" }
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
