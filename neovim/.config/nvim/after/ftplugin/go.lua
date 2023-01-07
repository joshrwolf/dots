vim.bo.expandtab = false
vim.bo.textwidth = 0
vim.bo.softtabstop = 0
vim.bo.tabstop = 4
vim.bo.shiftwidth = 4

-- Don't use treesitter, but {smart,auto}indent is good enoughtrue
vim.opt.smartindent = true
vim.opt.autoindent = true

vim.keymap.set("n", "<leader>dt", function ()
  require("dap-go").debug_test()
end, { desc = "Debug Test" })
