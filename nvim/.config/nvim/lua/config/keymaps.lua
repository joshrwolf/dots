-- Keymaps are automatically loaded on the VeryLazy event
-- Default keymaps that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/keymaps.lua
-- Add any additional keymaps here

-- Copied from: https://www.lazyvim.org/configuration/general#keymaps
local function map(mode, lhs, rhs, opts)
  local keys = require("lazy.core.handler").handlers.keys
  ---@cast keys LazyKeysHandler
  -- do not create the keymap if a lazy keys handler existsrt
  if not keys.active[keys.parse({ lhs, mode = mode }).id] then
    opts = opts or {}
    opts.silent = opts.silent ~= false
    if opts.remap and not vim.g.vscode then
      opts.remap = nil
    end
    vim.keymap.set(mode, lhs, rhs, opts)
  end
end

-- Smart splits
map("n", "<C-h>", function()
  require("smart-splits").move_cursor_left()
end, { remap = true })
map("n", "<C-j>", function()
  require("smart-splits").move_cursor_down()
end, { remap = true })
map("n", "<C-k>", function()
  require("smart-splits").move_cursor_up()
end, { remap = true })
map("n", "<C-l>", function()
  require("smart-splits").move_cursor_right()
end, { remap = true })

map("n", "<C-d>", "<C-d>zz", { remap = true })
map("n", "<C-u>", "<C-u>zz", { remap = true })
map("n", "n", "nzzzv", { remap = true })
map("n", "N", "Nzzzv", { remap = true })

-- Move Lines
-- TODO: This is causing me headache
map("n", "<A-j>", "", { desc = "Move down" })
map("n", "<A-k>", "", { desc = "Move up" })
map("i", "<A-j>", "", { desc = "Move down", remap = true })
map("i", "<A-k>", "", { desc = "Move up", remap = true })
map("v", "<A-j>", "", { desc = "Move down" })
map("v", "<A-k>", "", { desc = "Move up" })
