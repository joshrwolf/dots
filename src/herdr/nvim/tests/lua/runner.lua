-- mini.test owns collection/reporting; Cargo owns this process's deadline.
local ok, err = pcall(function()
  local tests = assert(vim.env.HERDR_NVIM_TESTS)
  vim.opt.runtimepath:append(assert(vim.env.MINI_TEST_PATH))
  package.path = tests .. "/lua/?.lua;" .. package.path
  local test = require("mini.test")
  test.setup({
    collect = {
      emulate_busted = false,
      find_files = function()
        return vim.fn.glob(tests .. "/lua/test_*.lua", false, true)
      end,
    },
    execute = { reporter = test.gen_reporter.stdout({ quit_on_finish = true }) },
  })
  local cases = test.collect()
  assert(#cases > 0, "no editor tests collected")
  test.execute(cases)
end)
if not ok then
  io.stderr:write(tostring(err) .. "\n")
  vim.cmd("cquit 1")
end
