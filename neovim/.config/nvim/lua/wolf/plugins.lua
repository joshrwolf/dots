return {
	{ "folke/which-key.nvim" },
	{
		"christoomey/vim-tmux-navigator",
		event = "VeryLazy",
		config = function()
			vim.g.tmux_navigator_save_on_switch = 1
		end,
	},
	{ "ThePrimeagen/harpoon" },
	{
		"tpope/vim-fugitive",
		cmd = { "Git" },
	},
	{
		"TimUntersberger/neogit",
		cmd = "Neogit",
		config = function()
			require("neogit").setup({
				kind = "replace",
				disable_insert_on_commit = false,
				disable_commit_confirmation = true,
				integrations = {
					diffview = true,
				},
			})
		end,
	},
	{
		"RRethy/vim-illuminate",
		event = "VeryLazy",
		config = function()
			require("illuminate").configure({
				providers = {
					"lsp",
					"treesitter",
					"regexp",
				},
			})
		end,
	},
	{
		"epwalsh/obsidian.nvim",
		cmd = { "ObsidianNew", "ObsidianOpen", "ObsidianToday", "ObsidianYesterday", "ObsidianSearch" },
		config = function()
			require("obsidian").setup({
				dir = "~/.brain",
				completion = {
					nvim_cmp = true,
				},
				daily_notes = { folder = "dailies" },
			})
		end,
	},
	{
		"m-demare/hlargs.nvim",
		event = "VeryLazy",
		config = function()
			local nf = require("nightfox.palette").load("nordfox")
			require("hlargs").setup({
				color = nf.white.dim,
			})
		end,
	},
}
