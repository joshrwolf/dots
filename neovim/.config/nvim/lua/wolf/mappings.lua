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
	[","] = { require("telescope.builtin").buffers, "Find Buffers" },
	["<space>"] = { require("telescope.builtin").find_files, "Find Files" },

	f = {
		name = "+file",
		f = { require("telescope.builtin").find_files, "File" },
		b = { require("telescope.builtin").buffers, "Buffers" },
		o = { require("telescope.builtin").oldfiles, "Old" },
		w = { require("telescope.builtin").live_grep, "Words" },
		h = { require("telescope.builtin").help_tags, "Help" },
		u = { "<cmd>lua require('telescope').extensions.undo.undo()<cr>", "Undos" },
		t = { "<cmd>TodoTelescope<cr>", "Todos" },
		["."] = { require("telescope.builtin").resume, "Resume" },
	},
	p = {
		name = "+plugins",
		i = { "<cmd>:Lazy<cr>", "Lazy" },
		m = { "<cmd>:Mason<cr>", "Mason" },
	},
	g = {
		name = "+git",
		g = { "<cmd>:0Git<cr>", "Git" },
		b = { require("telescope.builtin").git_branches, "Branches" },
	},

	j = { require("harpoon.ui").toggle_quick_menu, "Harpoon UI" },
	a = { require("harpoon.mark").add_file, "Add Harpoon File" },
	["1"] = { "<cmd>lua require('harpoon.ui').nav_file(1)<cr>", "Nav 1" },
	["2"] = { "<cmd>lua require('harpoon.ui').nav_file(2)<cr>", "Nav 2" },
}, { prefix = "<leader>" })

-- Register non leader keys
wk.register({
	["\\"] = { "<cmd>NnnPicker %:p:h<cr>", "Picker" },
	-- ["]q"] = { vim.}
	["[b"] = { "<cmd>:bprev<cr>", "Prev Buffer" },
	["H"] = { "<cmd>:bprev<cr>", "Prev Buffer" },
	["]b"] = { "<cmd>:bnext<cr>", "Next Buffer" },
	["L"] = { "<cmd>:bnext<cr>", "Next Buffer" },

	["]q"] = { "<cmd>:cnext<cr>", "Next Quickfix" },
	["[q"] = { "<cmd>:cprev<cr>", "Prev Quickfix" },
})
