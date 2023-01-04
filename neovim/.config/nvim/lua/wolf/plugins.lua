return {
	{ "folke/which-key.nvim" },
	{
		"christoomey/vim-tmux-navigator",
		event = "VeryLazy",
		config = function()
			vim.g.tmux_navigator_save_on_switch = 1
		end,
	},
	{
		-- Git
		"tpope/vim-fugitive",
		event = "VeryLazy",
		config = function()
			vim.api.nvim_create_autocmd("FileType", {
				pattern = "fugitive",
				callback = function(ctx)
					vim.keymap.set("n", "<Tab>", "=", { remap = true, buffer = ctx.buf })
					vim.keymap.set("n", "q", ":q<cr>", { buffer = ctx.buf })
					vim.keymap.set("n", "dt", ":Gtabedit <Plug><cfile><Bar>Gdiffsplit<CR>", { buffer = ctx.buf })
					vim.keymap.set("n", "S", "<cmd>silent !git add -A<cr>", { buffer = ctx.buf })
				end,
			})
		end,
	},
	{
		"RRethy/vim-illuminate",
		event = "BufReadPost",
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
		-- Note taking
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
		-- Adds special HL to args using treesitter
		"m-demare/hlargs.nvim",
		event = "VeryLazy",
		config = function()
			local nf = require("nightfox.palette").load("nordfox")
			require("hlargs").setup({
				color = nf.white.dim,
			})
		end,
	},
	{
		-- Surrounds
		"kylechui/nvim-surround",
		event = "VeryLazy",
		config = function()
			require("nvim-surround").setup()
		end,
	},
	{
		-- More matches
		"andymass/vim-matchup",
		event = "BufReadPost",
		config = function()
			vim.g.matchup_matchparen_offscreen = { method = nil }
		end,
		dependencies = {
			"nvim-treesitter/nvim-treesitter",
		},
	},
	{
		-- GH integration
		"pwntester/octo.nvim",
		cmd = { "Octo" },
		dependencies = {
			"nvim-lua/plenary.nvim",
			"nvim-telescope/telescope.nvim",
			"kyazdani42/nvim-web-devicons",
		},
		config = function()
			require("octo").setup({})
		end,
	},
}
