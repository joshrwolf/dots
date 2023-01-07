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
vim.keymap.set("n", "<C-j>", "<cmd>:TmuxNavigateDown<cr>", { silent = true })
vim.keymap.set("n", "<C-l>", "<cmd>:TmuxNavigateRight<cr>", { silent = true })
vim.keymap.set("n", "<C-k>", "<cmd>:TmuxNavigateUp<cr>", { silent = true })

vim.keymap.set("n", "J", "mzJ`z")
vim.keymap.set("n", "<C-d>", "<C-d>zz")
vim.keymap.set("n", "<C-u>", "<C-u>zz")
vim.keymap.set("n", "n", "nzzzv")
vim.keymap.set("n", "N", "Nzzzv")

-- Resize windows
vim.keymap.set("n", "<S-Up>", "<cmd>resize +2<cr>")
vim.keymap.set("n", "<S-Down>", "<cmd>resize -2<cr>")
vim.keymap.set("n", "<S-Left>", "<cmd>vertical resize -2<cr>")
vim.keymap.set("n", "<S-Right>", "<cmd>vertical resize +2<cr>")

-- Clear search highlights on esc
vim.keymap.set("n", "<esc>", ":noh<cr>", { noremap = true, silent = true })

-- Better indenting
vim.keymap.set("v", "<", "<gv")
vim.keymap.set("v", ">", ">gv")

-- Git Messenger
vim.keymap.set("n", "gk", ":GitMessenger<cr>", { silent = true })

-- Register leader keys
wk.register({
	["<space>"] = { require("telescope.builtin").find_files, "Find Files" },
	["q"] = { ":q<cr>", "Quit" },

	f = {
		name = "+file",
		f = { require("telescope.builtin").find_files, "File" },
		b = { require("telescope.builtin").buffers, "Buffers" },
		o = { require("telescope.builtin").oldfiles, "Old" },
		w = { require("telescope").extensions.live_grep_args.live_grep_args, "Words" },
		h = { require("telescope.builtin").help_tags, "Help" },
		u = { "<cmd>lua require('telescope').extensions.undo.undo()<cr>", "Undos" },
		t = { "<cmd>TodoTelescope<cr>", "Todos" },
		["."] = { require("telescope.builtin").resume, "Resume" },
		n = {
			name = "+notes",
			w = { "<cmd>ObsidianSearch<cr>", "Search Obsidian" },
			n = { "<cmd>ObsidianNew<cr>", "New Obsidian Note" },
			d = { "<cmd>ObsidianToday<cr>", "Daily Obsidian Note" },
			s = { "<cmd>ObsidianYesterday<cr>", "Yesterday's Obsidian Note" },
		},
	},
	p = {
		name = "+plugins",
		i = { "<cmd>:Lazy<cr>", "Lazy" },
		m = { "<cmd>:Mason<cr>", "Mason" },
	},
	g = {
		name = "+git",
		g = { ":tab G<cr>", "Git" },
		b = { require("telescope.builtin").git_branches, "Branches" },
		f = { "<cmd>DiffviewFileHistory %<cr>", "File History" },
		h = {
			name = "+github",
			y = { ":GBrowse!<cr>", "Copy permalink", mode = { "n", "v" } },
			Y = { ":GBrowse! @upstream<cr>", "Copy upstream permalink", mode = { "n", "v" } },
		},
		a = { ":Gwrite<cr>", "Write" },
	},
	t = {
		name = "+test",
		r = { "<cmd>lua require('neotest').run.run()<cr>", "Run Nearest" },
		f = { "<cmd>lua require('neotest').run.run(vim.fn.expand('%'))<cr>", "Run File" },
		l = { "<cmd>lua require('neotest').run.run_last()<cr>", "Run Last" },
		t = { "<cmd>lua require('neotest').summary.toggle()<cr>", "Toggle Summary" },
	},
}, { prefix = "<leader>" })

-- Register non leader keys
wk.register({
	["\\"] = { "<cmd>Telescope file_browser path=%:p:h<cr>", "Browser" },
	["<C-,>"] = { require("telescope.builtin").buffers, "Find Buffers" },
	g = {
		a = { ":A<cr>", "Goto Alternate" },
	},
})
