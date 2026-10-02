local T, child = require("support").child_set()
local eq = MiniTest.expect.equality

T["a reserved identity is used exactly and cannot be opened twice"] = function()
  eq(
    child.lua_func(function()
      local listener = require("dots.herdr.listener")
      local token = string.rep("a", 32)
      local first = listener.open(test_root .. "/r", token)
      local before = vim.fn.serverlist()
      local ok, err = pcall(listener.open, test_root .. "/r", token)
      local intact = vim.deep_equal(before, vim.fn.serverlist()) and vim.uv.fs_lstat(first.socket).type == "socket"
      first.close()
      return { first.token == token, first.socket == test_root .. "/r/" .. token .. ".sock", ok, err.code, intact }
    end),
    { true, true, false, "listener_unavailable", true }
  )
end

T["an existing pathname is never removed or replaced to make launch succeed"] = function()
  eq(
    child.lua_func(function()
      local directory = test_root .. "/r"
      vim.fn.mkdir(directory, "p", 448)
      local token = string.rep("b", 32)
      local path = directory .. "/" .. token .. ".sock"
      vim.fn.writefile({ "untouched" }, path)
      local before = vim.fn.serverlist()
      local ok, err = pcall(require("dots.herdr.listener").open, directory, token)
      return { ok, err.code, vim.fn.readfile(path), vim.deep_equal(before, vim.fn.serverlist()) }
    end),
    { false, "listener_unavailable", { "untouched" }, true }
  )
end

T["invalid reserved identities fail before creating resources"] = function()
  eq(
    child.lua_func(function()
      local directory = test_root .. "/not-created"
      local ok, err = pcall(require("dots.herdr.listener").open, directory, "../escape")
      return { ok, err.code, vim.uv.fs_lstat(directory) == nil }
    end),
    { false, "listener_unavailable", true }
  )
end

return T
