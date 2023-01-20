-- lazy
vim.keymap.set("n", "<leader>pi", "<cmd>:Lazy<cr>", { desc = "Lazy" })

-- Jumps
vim.keymap.set("n", "J", "mzJ`z")
vim.keymap.set("n", "<C-d>", "<C-d>zz")
vim.keymap.set("n", "<C-u>", "<C-u>zz")
vim.keymap.set("n", "n", "nzzzv")
vim.keymap.set("n", "N", "Nzzzv")

-- Resize windows
vim.keymap.set("n", "<S-Up>", "<cmd>resize +10<cr>")
vim.keymap.set("n", "<S-Down>", "<cmd>resize -10<cr>")
vim.keymap.set("n", "<S-Left>", "<cmd>vertical resize -10<cr>")
vim.keymap.set("n", "<S-Right>", "<cmd>vertical resize +10<cr>")

-- Clear search highlights on esc
vim.keymap.set("n", "<esc>", ":noh<cr>", { noremap = true, silent = true })

vim.keymap.set("v", "p", [["_dP]], { desc = "Keep the yanked text when pasting in visual  mode" })

-- Saner n and N
-- ref: https://github.com/mhinz/vim-galore#saner-behavior-of-n-and-n
vim.keymap.set("n", "n", "'Nn'[v:searchforward]", { expr = true, desc = "Next search result" })
vim.keymap.set("x", "n", "'Nn'[v:searchforward]", { expr = true, desc = "Next search result" })
vim.keymap.set("o", "n", "'Nn'[v:searchforward]", { expr = true, desc = "Next search result" })
vim.keymap.set("n", "N", "'nN'[v:searchforward]", { expr = true, desc = "Prev search result" })
vim.keymap.set("x", "N", "'nN'[v:searchforward]", { expr = true, desc = "Prev search result" })
vim.keymap.set("o", "N", "'nN'[v:searchforward]", { expr = true, desc = "Prev search result" })

-- Better indenting
vim.keymap.set("v", "<", "<gv")
vim.keymap.set("v", ">", ">gv")

vim.keymap.set("n", "<c-6>", "<c-^", { noremap = true })

-- Unimpaired "fixes"
vim.keymap.set("n", "[T", ":tabfirst<cr>", { desc = "First Tab" })
vim.keymap.set("n", "[t", ":tabp<cr>", { desc = "Prev Tab" })
vim.keymap.set("n", "]t", ":tabn<cr>", { desc = "Next Tab" })
vim.keymap.set("n", "]T", ":tablast<cr>", { desc = "Last Tab" })

-- Terminal
vim.keymap.set("t", "<esc><esc>", [[<C-\><C-n>]])

-- user cmd to open new terminal in current buffer's directory
vim.cmd(
	[[command! LocalTermVertical let s:term_dir=expand('%:p:h') | vertical new | call termopen([&shell], {'cwd': s:term_dir })]]
)
vim.cmd(
	[[command! LocalTerm let s:term_dir=expand('%:p:h') | new | resize 20 | call termopen([&shell], {'cwd': s:term_dir })]]
)
vim.keymap.set("n", "<leader>ot", ":LocalTerm<cr>", { desc = "Open" })
vim.keymap.set("n", "<c-;>", ":LocalTermVertical<cr>", { desc = "New Vertical Terminal" })
vim.keymap.set("n", "<c-.>", ":LocalTerm<cr>", { desc = "New Horizontal Terminal" })
