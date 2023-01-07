return {
	{
		"folke/which-key.nvim",
		event = "VeryLazy",
		config = function()
			require("which-key").setup({
				show_help = false,
				key_labels = { ["<leader>"] = "SPC" },
				triggers = "auto",
			})
		end,
	},
	{
		"christoomey/vim-tmux-navigator",
		event = "VeryLazy",
		config = function()
			vim.g.tmux_navigator_save_on_switch = 1
		end,
	},
	{
		"tpope/vim-unimpaired",
		event = "VeryLazy",
	},
	{
		"tpope/vim-projectionist",
		cmd = { "A", "AV", "AS" },
		event = "VeryLazy",
		config = function()
			-- TODO: There's probably a smarter way to do this with lua
			vim.cmd([[
let g:projectionist_heuristics = {
  \ '*.go': {
  \   '*.go': {
  \       'alternate': '{}_test.go',
  \       'type': 'source'
  \   },
  \   '*_test.go': {
  \       'alternate': '{}.go',
  \       'type': 'test'
  \   },
  \ }}
]])
		end,
	},
	{
		-- Git
		"tpope/vim-fugitive",
		event = "VeryLazy",
		dependencies = {
			"tpope/vim-rhubarb",
			"rhysd/git-messenger.vim",
		},
		config = function()
			vim.api.nvim_create_autocmd("FileType", {
				pattern = "fugitive",
				callback = function(ctx)
					vim.keymap.set("n", "<Tab>", "=", { remap = true, buffer = ctx.buf })
					vim.keymap.set("n", "q", ":q<cr>", { buffer = ctx.buf })
					vim.keymap.set("n", "dt", ":Gtabedit <Plug><cfile><Bar>Gdiffsplit<CR>", { buffer = ctx.buf })
					vim.keymap.set("n", "S", "<cmd>silent !git add -A<cr>", { buffer = ctx.buf, desc = "Add All" })
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
			"nvim-tree/nvim-web-devicons",
		},
		config = function()
			require("octo").setup({})
		end,
	},
	{
		-- Better yanking
		"gbprod/yanky.nvim",
		event = "BufReadPost",
		config = function()
			require("yanky").setup({
				highlight = { timer = 250 },
			})

			vim.keymap.set({ "n", "x" }, "y", "<Plug>(YankyYank)")
			vim.keymap.set({ "n", "x" }, "p", "<Plug>(YankyPutAfter)")
			vim.keymap.set({ "n", "x" }, "P", "<Plug>(YankyPutBefore)")
			vim.keymap.set({ "n", "x" }, "gp", "<Plug>(YankyGPutAfter)")
			vim.keymap.set({ "n", "x" }, "gP", "<Plug>(YankyGPutBefore)")

			vim.keymap.set("n", "<C-n>", "<Plug>(YankyCycleForward)")
			vim.keymap.set("n", "<C-p>", "<Plug>(YankyCycleBackward)")

			vim.keymap.set(
				"n",
				"<leader>fy",
				"<cmd>lua require('telescope').extensions.yank_history.yank_history()<cr>"
			)
		end,
	},
}
