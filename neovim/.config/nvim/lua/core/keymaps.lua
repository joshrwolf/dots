-- local keymap = vim.api.nvim_set_keymap
local keymap = vim.keymap.set
local dopts = { noremap = true, silent = true }
local eopts = { noremap = true, expr = true, silent = true }

-- Window movement
-- keymap("n", "<C-h>", "<C-w>h", dopts)
-- keymap("n", "<C-j>", "<C-w>j", dopts)
-- keymap("n", "<C-k>", "<C-w>k", dopts)
-- keymap("n", "<C-l>", "<C-w>l", dopts)
-- keymap("t", "<C-h>", "<C-\\><C-N><C-w>h", )

-- Don't move when *'ing
keymap("n", "*", "*N", dopts)

-- Keep cursor center when nN'ing
keymap("n", "n", "nzz", dopts)
keymap("n", "N", "Nzz", dopts)

-- -- Keep cursor centered when paging
-- keymap("n", "<C-d>", "<cmd>lua Scroll('<C-d>', 1, 1)<CR>zz", dopts)
-- keymap("n", "<C-u>", "<cmd>lua Scroll('<C-u>', 1, 1)<CR>zz", dopts)

-- Center search results
keymap("n", "k", "v:count == 0 ? 'gk' : 'k'", eopts)
keymap("n", "j", "v:count == 0 ? 'gj' : 'j'", eopts)

-- Better indent
keymap("v", "<", "<gv", dopts)
keymap("v", ">", ">gv", dopts)

-- Paste over currently select text without yanking it
keymap("v", "p", '"_dp', dopts)
keymap("v", "P", '"_dP', dopts)

-- Switch buffer
keymap("n", "<S-h>", ":bprevious<CR>", dopts)
keymap("n", "<S-l>", ":bnext<CR>", dopts)

-- -- Move over a closing element in insert mode
keymap("i", "<C-l>", function()
	return require("utils").escapePair()
end, dopts)

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

keymap("n", "<C-p>", "<cmd>lua require('telescope.builtin').find_files()<cr>", dopts)

-- Open links under the cursor
if vim.fn.has("macunix") == 1 then
	keymap("n", "gx", "<cmd>silent execute '!open ' . shellescape('<cWORD>')<CR>", dopts)
else
	keymap("n", "gx", "<cmd>silent execute '!xdg-open ' . shellescape('<cWORD>')<CR>", dopts)
end
