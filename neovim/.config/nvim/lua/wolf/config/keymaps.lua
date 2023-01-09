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
vim.keymap.set("n", "<c-;>", "<cmd>vsplit | terminal<cr>", { desc = "New Vertical Terminal" })
vim.keymap.set("n", "<c-.>", "<cmd>split | resize 20 | terminal<cr>", { desc = "New Horizontal Terminal" })
vim.keymap.set("t", "<esc>", [[<C-\><C-n>]])
