-- Options are automatically loaded before lazy.nvim startup
-- Default options that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/options.lua
-- Add any additional options here

vim.g.lazyvim_picker = "snacks"
vim.g.snacks_animate = false

vim.opt.jumpoptions = "view" -- restore location when jumping through buffers
vim.opt.swapfile = false
vim.opt.writebackup = false
vim.opt.autoread = true -- auto reload file when changed outside of vim

vim.g.lazyvim_python_lsp = "basedpyright"
