local T, child = require("support").child_set()
local eq = MiniTest.expect.equality

T["current pane uses bounded caller discovery rather than inherited IDs"] = function()
  eq(child.lua_func(function()
    local host = require("dots.review.host").new(vim.env.HERDR_SOCKET_PATH, function(args, options)
      assert(vim.deep_equal(args, { "herdr", "pane", "current", "--current" }))
      assert(options.timeout == 1500 and options.text)
      return { wait = function(_, timeout)
        assert(timeout == 1500)
        return { code = 0, stdout = vim.json.encode({ result = {
          type = "pane_current", pane = { pane_id = "w2:p3", tab_id = "w2:t1", workspace_id = "w2" },
        } }) }
      end }
    end)
    return host.current()
  end), { pane = "w2:p3", tab = "w2:t1", workspace = "w2" })
end

T["failed commands and invalid identities are rejected"] = function()
  eq(child.lua_func(function()
    for _, result in ipairs({
      { code = 1, stderr = "unavailable" },
      { code = 0, stdout = "invalid JSON" },
      { code = 0, stdout = "{}" },
      { code = 0, stdout = vim.json.encode({ result = { type = "pane_current", pane = {
        pane_id = "", tab_id = "t1", workspace_id = "w1",
      } } }) },
    }) do
      local host = require("dots.review.host").new(vim.env.HERDR_SOCKET_PATH, function()
        return { wait = function() return result end }
      end)
      assert(not pcall(host.current))
    end
    local host = require("dots.review.host").new("/tmp/other.sock", function()
      error("must not execute against a changed endpoint")
    end)
    assert(not pcall(host.current))
    return true
  end), true)
end

return T
