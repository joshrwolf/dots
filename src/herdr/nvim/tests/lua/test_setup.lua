local T, child = require("support").child_set()

T["setup is inert outside Herdr and can be repeated"] = function()
  MiniTest.expect.equality(
    child.lua_func(function()
      local endpoints = vim.fn.serverlist()
      local editor = require("dots.herdr")
      editor.setup()
      editor.setup()
      vim.cmd("HerdrEditorRegister")
      local reply = vim.json.decode(editor.request("{}"))
      return { vim.fn.exists(":HerdrEditorRegister"), vim.deep_equal(endpoints, vim.fn.serverlist()), reply.error.code }
    end),
    { 2, true, "not_attached" }
  )
end

T["headless, nested and opted-out editors cannot register"] = function()
  local eq = MiniTest.expect.equality
  child.lua_func(function()
    vim.env.HERDR_ENV = "1"
    vim.env.HERDR_PANE_ID = "w1:p1"
    editor = require("dots.herdr")
    endpoints = vim.fn.serverlist()
  end)
  eq(child.lua_get("editor.should_register()"), false)
  eq(child.lua_get("editor.register()"), false)
  require("support").ui(child)
  eq(child.lua_get("editor.should_register()"), true)
  eq(
    child.lua_func(function()
      vim.env.NVIM = "/tmp/parent-editor.sock"
      return { editor.should_register(), editor.register() }
    end),
    { false, false }
  )
  eq(
    child.lua_func(function()
      vim.env.NVIM = nil
      vim.env.DOTS_HERDR_EDITOR = "0"
      return { editor.should_register(), editor.register() }
    end),
    { false, false }
  )
  eq(
    child.lua_func(function()
      vim.env.DOTS_HERDR_EDITOR = nil
      vim.g.dots_herdr_editor = false
      return { editor.should_register(), editor.register(), vim.deep_equal(endpoints, vim.fn.serverlist()) }
    end),
    { false, false, true }
  )
end

return T
