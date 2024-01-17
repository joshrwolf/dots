-- Options are automatically loaded before lazy.nvim startup
-- Default options that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/options.lua
-- Add any additional options here

local opt = vim.opt

opt.confirm = false
opt.showtabline = 1 -- show tabline only when multiple tabs exist
opt.jumpoptions = "view" -- restore location when jumping through buffers

opt.updatetime = 250 -- time before CursorHold is trigger
opt.swapfile = false
opt.writebackup = false
opt.undofile = true
opt.autoread = true -- auto reload file when changed outside of vim

require("config.hide")
