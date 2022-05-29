local map = vim.keymap.set
local dopts = { silent = true }
local expr_options = { expr = true, silent = true }

-- Remap leader key to space
map({ "n", "v" }, "<Space>", "<Nop>", { silent = true })
vim.g.mapleader = " "

-- Move around splits using Ctrl + {h,j,k,l}
map("n", "<C-h>", "<C-w>h", dopts)
map("n", "<C-j>", "<C-w>j", dopts)
map("n", "<C-k>", "<C-w>k", dopts)
map("n", "<C-l>", "<C-w>l", dopts)

-- Reload configuration without restaring neovim
map("n", "<leader>r", ":so %<CR>", dopts)

-- Better indenting
map("v", "<", "<gv", dopts)
map("v", ">", ">gv", dopts)

-- Paste over selected text without yanking it
map("v", "p", '"_dP', dopts)

