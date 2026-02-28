-- Keymaps are automatically loaded on the VeryLazy event
-- Default keymaps that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/keymaps.lua
-- Add any additional keymaps here

vim.keymap.set({ "n" }, "<C-d>", "<C-d>zz", { remap = true })
vim.keymap.set({ "n" }, "<C-u>", "<C-u>zz", { remap = true })

-- Disable Move Lines
-- TODO: This is causing me headache
vim.keymap.del({ "n", "i", "v" }, "<A-j>")
vim.keymap.del({ "n", "i", "v" }, "<A-k>")

vim.keymap.set("n", "<c-/>", function()
  LazyVim.terminal()
end, { desc = "Open LazyVim Terminal" })

-- Tab navigation
-- vim.keymap.set("n", "[t", "<cmd>tabprevious<cr>", { desc = "Previous Tab" })
-- vim.keymap.set("n", "]t", "<cmd>tabnext<cr>", { desc = "Next Tab" })


