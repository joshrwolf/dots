-- Options are automatically loaded before lazy.nvim startup
-- Default options that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/options.lua
-- Add any additional options here

local opt = vim.opt

opt.confirm = false
opt.showtabline = 1 -- show tabline only when multiple tabs exist
opt.jumpoptions = "view" -- restore location when jumping through buffers

-- opt.autoindent = true
-- opt.smartindent = false
-- opt.cindent = true
-- opt.indentexpr = "nvim_treesitter#indent()"

-- vim.lsp.set_log_level("OFF")
