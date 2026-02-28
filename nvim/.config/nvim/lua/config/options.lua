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

-- Use OSC 52 for clipboard when over SSH (dev VMs) — passes through tmux to Mac clipboard
if os.getenv("SSH_TTY") then
  vim.opt.clipboard = "unnamedplus"
  vim.g.clipboard = {
    name = "OSC 52",
    copy = {
      ["+"] = require("vim.ui.clipboard.osc52").copy("+"),
      ["*"] = require("vim.ui.clipboard.osc52").copy("*"),
    },
    paste = {
      ["+"] = function() return { vim.fn.split(vim.fn.getreg(""), "\n"), vim.fn.getregtype("") } end,
      ["*"] = function() return { vim.fn.split(vim.fn.getreg(""), "\n"), vim.fn.getregtype("") } end,
    },
  }
end
