local wk = require("which-key")

wk.setup({
	show_help = false,
	triggers = "auto",
	plugins = { spelling = true },
})

vim.keymap.set({ "n", "v" }, "<Space>", "<Nop>", { silent = true })

vim.keymap.set("n", "<C-h>", "<C-w>h")
vim.keymap.set("n", "<C-j>", "<C-w>j")
vim.keymap.set("n", "<C-k>", "<C-w>k")
vim.keymap.set("n", "<C-l>", "<C-w>l")

vim.keymap.set("n", "<C-h>", "<cmd>:TmuxNavigateLeft<cr>", { silent = true })
vim.keymap.set("n", "<C-l>", "<cmd>:TmuxNavigateRight<cr>", { silent = true })

vim.keymap.set("n", "J", "mzJ`z")
vim.keymap.set("n", "<C-d>", "<C-d>zz")
vim.keymap.set("n", "<C-u>", "<C-u>zz")
vim.keymap.set("n", "n", "nzzzv")
vim.keymap.set("n", "N", "Nzzzv")

-- Better indenting
vim.keymap.set("v", "<", "<gv")
vim.keymap.set("v", ">", ">gv")

-- Register leader keys
wk.register({
	f = {
		name = "+file",
		f = { require("telescope.builtin").find_files, "Find file" },
		u = { "<cmd>lua require('telescope').extensions.undo.undo()<cr>", "Find undos" },
		t = { "<cmd>TodoTelescope<cr>", "Todos" },
	},
	p = {
		name = "+plugins",
		i = { "<cmd>:Lazy<cr>", "Lazy" },
		m = { "<cmd>:Mason<cr>", "Mason" },
	},
	g = {
		name = "+git",
		g = { "<cmd>:G<cr>", "Git" },
		b = { require("telescope.builtin").git_branches, "Branches" },
	},
}, { prefix = "<leader>" })

-- Register non leader keys
wk.register({
	["\\"] = { "<cmd>NnnPicker<cr>", "Picker" },
	-- ["]q"] = { vim.}
	["[b"] = { "<cmd>:bprev<cr>", "Prev Buffer" },
	["H"] = { "<cmd>:bprev<cr>", "Prev Buffer" },
	["]b"] = { "<cmd>:bnext<cr>", "Next Buffer" },
	["L"] = { "<cmd>:bnext<cr>", "Next Buffer" },
})
