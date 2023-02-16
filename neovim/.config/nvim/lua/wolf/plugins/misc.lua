return {
	{
		"christoomey/vim-tmux-navigator",
		keys = {
			{ "<c-h>", "<cmd>:TmuxNavigateLeft<cr>", "n", desc = "Navigate Left" },
			{ "<c-j>", "<cmd>:TmuxNavigateUp<cr>", "n", desc = "Navigate Up" },
			{ "<c-k>", "<cmd>:TmuxNavigateDown<cr>", "n", desc = "Navigate Down" },
			{ "<c-l>", "<cmd>:TmuxNavigateRight<cr>", "n", desc = "Navigate Right" },
		},
		config = function()
			vim.g.tmux_navigator_no_mappings = 1
			vim.g.tmux_navigator_save_on_switch = 1
		end,
	},
	{
		"echasnovski/mini.comment",
		keys = {
			{ "gc", mode = { "n", "v", "o" } },
			{ "gcc", mode = { "n" } },
		},
		opts = {},
		config = function(_, opts)
			require("mini.comment").setup(opts)
		end,
	},
	{
		"michaeljsmith/vim-indent-object",
		keys = {
			{ "ii", mode = { "x", "o" } },
			{ "ai", mode = { "x", "o" } },
			{ "aI", mode = { "x", "o" } },
			{ "iI", mode = { "x", "o" } },
		},
	},
	{
		"kylechui/nvim-surround",
		event = "VeryLazy",
		config = true,
	},
	{
		"RRethy/vim-illuminate",
		event = "BufReadPost",
		config = function()
			require("illuminate").configure({
				delay = 200,
			})
		end,
		keys = {
			{
				"]]",
				function()
					require("illuminate").goto_next_reference(false)
				end,
				desc = "Next Reference",
			},
			{
				"[[",
				function()
					require("illuminate").goto_prev_reference(false)
				end,
				desc = "Prev Reference",
			},
		},
	},
	{
		"folke/which-key.nvim",
		version = "*",
		event = "VeryLazy",
		config = function()
			local wk = require("which-key")
			wk.setup({
				plugins = { spelling = true },
				key_labels = { ["<leader>"] = "SPC" },
			})
			wk.register({
				mode = { "n", "v" },
				["g"] = { name = "+goto" },
				["]"] = { name = "+next" },
				["["] = { name = "+prev" },
				["<leader>f"] = {
					name = "+file",
					n = { name = "+notes" },
				},
				["<leader>g"] = {
					name = "+git",
					h = { name = "+github" },
				},
				["<leader>c"] = { name = "+code" },
				["<leader>d"] = { name = "+debug" },
				["<leader>t"] = { name = "+test" },
			})
		end,
	},
	{
		"lukas-reineke/indent-blankline.nvim",
		version = "*",
		event = "BufReadPre",
		opts = {
			buftype_exclude = { "terminal", "nofile" },
			show_current_context = true,
			show_current_context_start = false,
			use_treesitter = true,
			show_end_of_line = true,
		},
	},
	-- {
	-- 	"folke/noice.nvim",
	-- 	event = "VeryLazy",
	-- 	dependencies = {
	-- 		"MunifTanjim/nui.nvim",
	-- 	},
	-- 	opts = {
	-- 		lsp = {
	-- 			-- override markdown rendering so that **cmp** and other plugins use **Treesitter**
	-- 			override = {
	-- 				["vim.lsp.util.convert_input_to_markdown_lines"] = true,
	-- 				["vim.lsp.util.stylize_markdown"] = true,
	-- 				["cmp.entry.get_documentation"] = true,
	-- 			},
	-- 		},
	-- 		presets = {
	-- 			bottom_search = false,
	-- 			command_palette = false,
	-- 			long_message_to_split = false,
	-- 			inc_rename = false,
	-- 		},
	-- 		cmdline = {
	-- 			view = "cmdline",
	-- 		},
	-- 		notify = { enabled = false },
	-- 	},
	-- },
	{
		"stevearc/dressing.nvim",
		lazy = true,
		init = function()
			---@diagnostic disable-next-line: duplicate-set-field
			vim.ui.select = function(...)
				require("lazy").load({ plugins = { "dressing.nvim" } })
				return vim.ui.select(...)
			end
			---@diagnostic disable-next-line: duplicate-set-field
			vim.ui.input = function(...)
				require("lazy").load({ plugins = { "dressing.nvim" } })
				return vim.ui.input(...)
			end
		end,
	},
	{
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
		-- Adds special HL to args using treesitter
		"m-demare/hlargs.nvim",
		event = "BufReadPost",
		dependencies = {
			"nvim-treesitter/nvim-treesitter",
		},
		config = function()
			local nf = require("nightfox.palette").load("nordfox")
			require("hlargs").setup({
				color = nf.white.dim,
			})
		end,
	},
	{
		"tpope/vim-rsi",
		event = "InsertEnter",
	},
	{
		"tpope/vim-unimpaired",
		version = "*",
		event = "BufReadPost",
	},
	{
		"tpope/vim-projectionist",
		keys = {
			{ "ga", "<cmd>:A<cr>", "Alternate" },
		},
		event = "BufReadPost",
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
  \   '*.proto': {
  \       'alternate': '{}.pb.go',
  \       'type': 'source'
  \   },
  \   '*.pb.go': {
  \       'alternate': '{}.proto',
  \       'type': 'generated'
  \   },
  \ }}
]])
		end,
	},
	{
		-- Note taking
		"epwalsh/obsidian.nvim",
		version = "*",
		keys = {
			{ "<leader>fnw", "<cmd>ObsidianSearch<cr>", "Search Obsidian" },
			{ "<leader>fnn", "<cmd>ObsidianNew<cr>", "New Notes" },
			{ "<leader>fnd", "<cmd>ObsidianToday<cr>", "Daily Note" },
			{ "<leader>fns", "<cmd>ObsidianYesterday<cr>", "Yesterday Note" },
		},
		opts = {
			dir = "~/.brain",
			completion = {
				nvim_cmp = true,
			},
			daily_notes = { folder = "dailies" },
		},
	},
	{
		"folke/todo-comments.nvim",
		cmd = { "TodoTelescope" },
		event = "BufReadPost",
		config = true,
		keys = {
			{ "<leader>fT", "<cmd>TodoTelescope<cr>", desc = "Todo Telescope" },
			{
				"[x",
				function()
					require("todo-comments").jump_prev()
				end,
				desc = "Prev Todo",
			},
			{
				"]x",
				function()
					require("todo-comments").jump_next()
				end,
				desc = "Next Todo",
			},
		},
	},
	{
		"utilyre/barbecue.nvim",
		name = "barbecue",
		version = "*",
		event = "BufReadPost",
		dependencies = {
			"neovim/nvim-lspconfig",
			"SmiteshP/nvim-navic",
			"nvim-tree/nvim-web-devicons",
		},
		opts = {
			attach_navic = false,
			show_modified = true,
			symbols = {
				separator = "",
			},
			kinds = require("wolf.config.settings").icons.kinds,
			theme = {
				dirname = { fg = require("nightfox.palette").load("nordfox").comment },
				separator = { fg = require("nightfox.palette").load("nordfox").comment },
			},
		},
	},
	{
		"petertriho/nvim-scrollbar",
		event = "BufReadPost",
		opts = {
			"prompt",
			"TelescopePrompt",
			"noice",
			"notify",
		},
	},
	{
		"romainchapou/nostalgic-term.nvim",
		lazy = false,
		config = function()
			require("nostalgic-term").setup({
				mappings = {
					{ "<c-h>", "h" },
					{ "<c-j>", "j" },
					{ "<c-k>", "k" },
					{ "<c-l>", "l" },
				},
			})
		end,
	},
	{
		"ggandor/leap.nvim",
		event = "VeryLazy",
		dependencies = {
			"tpope/vim-repeat",
		},
		config = function()
			require("leap").add_default_mappings()
		end,
	},
	{
		"ggandor/flit.nvim",
		event = "VeryLazy",
		dependencies = {
			"ggandor/leap.nvim",
		},
		opts = {
			multiline = true,
		},
	},
	{
		"ThePrimeagen/refactoring.nvim",
		dependencies = {
			"nvim-lua/plenary.nvim",
			"nvim-treesitter/nvim-treesitter",
		},
	},
	{
		"stevearc/oil.nvim",
		cmd = { "Oil" },
		opts = {
			view_options = {
				show_hidden = true,
			},
		},
	},
	{
		"stevearc/overseer.nvim",
		cmd = { "OverseerRun", "Overseer" },
		opts = {},
	},
	{
		"stevearc/resession.nvim",
		keys = {
			{ "<leader>ss", "<cmd>lua require('resession').save()<cr>", "n", desc = "Save Session" },
			{ "<leader>sl", "<cmd>lua require('resession').load()<cr>", "n", desc = "Load Session" },
		},
		opts = {},
	},
	{
		"ThePrimeagen/harpoon",
		keys = {
			{ "<leader>0", "<cmd>lua require('harpoon.term').gotoTerminal(1)<cr>", "n", desc = "Goto Terminal 1" },
			{ "<leader>9", "<cmd>lua require('harpoon.term').gotoTerminal(2)<cr>", "n", desc = "Goto Terminal 2" },
		},
	},
	{
		"dstein64/vim-startuptime",
		cmd = "StartupTime",
		config = function()
			vim.g.startuptime_tries = 20
		end,
	},
}
