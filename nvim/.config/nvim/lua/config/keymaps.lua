-- Keymaps are automatically loaded on the VeryLazy event
-- Default keymaps that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/keymaps.lua
-- Add any additional keymaps here

local util = require("util")

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

local nav = {
  h = "Left",
  j = "Down",
  k = "Up",
  l = "Right",
}

-- Support wezterm movements
local function navigate(dir)
  return function()
    local win = vim.api.nvim_get_current_win()
    vim.cmd.wincmd(dir)
    local pane = vim.env.WEZTERM_PANE
    if pane and win == vim.api.nvim_get_current_win() then
      local pane_dir = nav[dir]
      vim.system({ "wezterm", "cli", "activate-pane-direction", pane_dir }, { text = true }, function(p)
        if p.code ~= 0 then
          vim.notify(
            "Failed to move to pane " .. pane_dir .. "\n" .. p.stderr,
            vim.log.levels.ERROR,
            { title = "Wezterm" }
          )
        end
      end)
    end
  end
end

util.set_user_var("IS_NVIM", true)
vim.api.nvim_create_autocmd({ "VimSuspend", "VimLeave" }, {
  pattern = "*",
  callback = function()
    util.set_user_var("IS_NVIM", false)
  end,
  desc = "unset IS_NVIM",
})
vim.api.nvim_create_autocmd({ "VimResume" }, {
  pattern = "*",
  callback = function()
    util.set_user_var("IS_NVIM", true)
  end,
  desc = "reset IS_NVIM",
})

-- Move to window using the movement keys
for key, dir in pairs(nav) do
  vim.keymap.set("n", "<" .. dir .. ">", navigate(key))
  vim.keymap.set("n", "<C-" .. key .. ">", navigate(key))
end

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
