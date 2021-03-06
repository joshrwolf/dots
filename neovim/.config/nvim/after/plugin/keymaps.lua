local keymap = vim.api.nvim_set_keymap
local dopts = { noremap = true, silent = true }
local eopts = { noremap = true, expr = true, silent = true }

-- Window movement
-- keymap("n", "<C-h>", "<C-w>h", dopts)
-- keymap("n", "<C-j>", "<C-w>j", dopts)
-- keymap("n", "<C-k>", "<C-w>k", dopts)
-- keymap("n", "<C-l>", "<C-w>l", dopts)
-- keymap("t", "<C-h>", "<C-\\><C-N><C-w>h", )

-- Better escaping using jk in insert and terminal mode
keymap("i", "jk", "<ESC>", dopts)
-- keymap("t", "jk", "<C-\\><C-n>", dopts)

-- Center search results
keymap("n", "k", "v:count == 0 ? 'gk' : 'k'", eopts)
keymap("n", "j", "v:count == 0 ? 'gj' : 'j'", eopts)

-- Better indent
keymap("v", "<", "<gv", dopts)
keymap("v", ">", ">gv", dopts)

-- Paste over currently select text without yanking it
keymap("v", "p", '"_dP', dopts)

-- Switch buffer
keymap("n", "<S-h>", ":bprevious<CR>", dopts)
keymap("n", "<S-l>", ":bnext<CR>", dopts)

-- Cancel search highlights with ESC
keymap("n", "<ESC>", ":nohlsearch<Bar>:echo<CR>", dopts)

-- Move selected line / block of text in visual mode
keymap("x", "K", ":move '<-2<CR>gv-gv", dopts)
keymap("x", "J", ":move '>+1<CR>gv-gv", dopts)

-- Resize Panes
keymap("n", "<Left>", ":vertical resize +5<CR>", dopts)
keymap("n", "<Right>", ":vertical resize -5<CR>", dopts)
keymap("n", "<Up>", ":resize -3<CR>", dopts)
keymap("n", "<Down>", ":resize +3<CR>", dopts)

