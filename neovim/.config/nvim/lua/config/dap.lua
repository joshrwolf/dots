local M = {}

local whichkey = require("which-key")

function M.setup()
  require("dapui").setup()

  whichkey.register({
    d = {
      name = "Debug",
      t = { "<cmd>lua require('dap').toggle_breakpoint()<cr>", "Toggle Breakpoint" },
      s = { "<cmd>lua require('dap').continue()<cr>", "Start" },
      q = { "<cmd>lua require('dap').close()<cr>", "Quit" },
    },
  }, { prefix = "<leader>" })
end

return M
