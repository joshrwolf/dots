-- Native headless/UI semantics, before implementing an editor owner. No Herdr
-- session, user config, fake UI API, or process adoption is involved.
local support = require("support")
local T, child = support.child_set()
local eq = MiniTest.expect.equality

local function prepare()
  child.lua_func(function()
    local path = test_root .. "/work.txt"
    vim.fn.writefile({ "saved", "second", "third" }, path)
    vim.cmd("edit " .. vim.fn.fnameescape(path))
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "unsaved", "second", "third" })
    vim.api.nvim_win_set_cursor(0, { 2, 2 })
    local hidden = vim.fn.bufadd(test_root .. "/hidden.txt")
    vim.fn.bufload(hidden)
    vim.bo[hidden].buflisted = true
    vim.api.nvim_buf_set_lines(hidden, 0, -1, false, { "unsaved hidden buffer" })
    vim.fn.setreg("a", "preserve register")
    function state()
      return {
        pid = vim.fn.getpid(),
        socket = vim.v.servername,
        buffer = vim.api.nvim_get_current_buf(),
        path = vim.api.nvim_buf_get_name(0),
        text = vim.api.nvim_buf_get_lines(0, 0, -1, false),
        dirty = vim.bo.modified,
        cursor = vim.api.nvim_win_get_cursor(0),
        undo = vim.fn.undotree().seq_cur,
        hidden = { vim.bo[hidden].modified, vim.api.nvim_buf_get_lines(hidden, 0, -1, false) },
        register = vim.fn.getreg("a"),
      }
    end
  end)
end

local function wait_uis(count)
  assert(
    vim.wait(2000, function()
      return #child.api.nvim_list_uis() == count
    end, 10),
    "unexpected attached UI count"
  )
end

local function detach(ui)
  -- Exercise the command through the intended UI, not a control RPC whose
  -- channel is unrelated to the terminal that should be detached.
  vim.fn.chansend(ui.job, ":detach\r")
  assert(vim.fn.jobwait({ ui.job }, 2000)[1] ~= -1, "UI did not detach")
end

T["unexpected UI disconnect preserves visible and hidden unsaved state"] = function()
  prepare()
  local before = child.lua_get("state()")
  local first = support.ui(child)
  eq(child.lua_get("state()"), before)
  first.stop()
  wait_uis(0)
  eq(child.lua_get("state()"), before)
  local second = support.ui(child)
  eq(second.channel == first.channel, false)
  eq(child.lua_get("state()"), before)
  second.stop()
  wait_uis(0)
  eq(child.lua_get("state()"), before)
end

T["native detach preserves the editor and permits reattachment"] = function()
  prepare()
  local before = child.lua_get("state()")
  local first = support.ui(child)
  detach(first)
  wait_uis(0)
  eq(child.lua_get("state()"), before)
  support.ui(child)
  eq(child.lua_get("state()"), before)
end

T["detaching one UI does not disconnect another"] = function()
  prepare()
  local first, second = support.ui(child), support.ui(child)
  local before = child.lua_get("state()")
  detach(first)
  wait_uis(1)
  eq(child.api.nvim_list_uis()[1].chan, second.channel)
  eq(child.lua_get("state()"), before)
end

T["multiple UIs share editor focus rather than independent views"] = function()
  prepare()
  local first, second = support.ui(child), support.ui(child)
  vim.fn.chansend(first.job, "gg")
  assert(vim.wait(2000, function()
    return child.api.nvim_win_get_cursor(0)[1] == 1
  end, 10))
  vim.fn.chansend(second.job, "G")
  assert(vim.wait(2000, function()
    return child.api.nvim_win_get_cursor(0)[1] == 3
  end, 10))
  eq(#child.api.nvim_list_uis(), 2)
end

T["ordinary quit refuses an unsaved editor"] = function()
  prepare()
  support.ui(child)
  eq(
    child.lua_func(function()
      local ok, err = pcall(vim.cmd, "quit")
      return { ok, tostring(err):match("E%d+"), vim.bo.modified }
    end),
    { false, "E37", true }
  )
end

T["quit-all is an editor operation, not a UI hide operation"] = function()
  prepare()
  local ui = support.ui(child)
  -- Force is used only to demonstrate the destructive native behavior on this
  -- disposable test process. An owner must never implement hide this way.
  vim.fn.chansend(ui.job, ":qall!\r")
  assert(vim.fn.jobwait({ child.job.id }, 2000)[1] ~= -1, "qall! did not stop the editor")
  assert(vim.fn.jobwait({ ui.job }, 2000)[1] ~= -1, "UI did not notice editor exit")
end

return T
