local expect = require("tests.expect")
local must = require("dots.result").must

return {
  ["returns the value"] = function()
    expect.equal(must("reading", { id = 1 }), { id = 1 })
  end,

  ["raises the error with its context"] = function()
    expect.fails(function()
      must("opening view", nil, "conflict")
    end, "opening view: conflict")
  end,

  ["names a missing error"] = function()
    expect.fails(function()
      must("opening view", nil)
    end, "opening view: unknown error")
  end,
}
