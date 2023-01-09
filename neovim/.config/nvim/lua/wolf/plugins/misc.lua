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
	-- {
	-- 	"echasnovski/mini.ai",
	-- 	version = false,
	-- 	-- keys = {
	-- 	-- 	{ "a", mode = { "x", "o" } },
	-- 	-- 	{ "i", mode = { "x", "o" } },
	-- 	-- },
	-- 	event = "VeryLazy",
	-- 	enabled = true,
	-- 	dependencies = {
	-- 		"nvim-treesitter/nvim-treesitter-textobjects",
	-- 		init = function()
	-- 			-- we don't actually load the plugin, just the module
	-- 			require("lazy.core.loader").disable_rtp_plugin("nvim-treesitter-textobjects")
	-- 		end,
	-- 	},
	-- 	config = function()
	-- 		local ai = require("mini.ai")
	-- 		ai.setup({
	-- 			n_lines = 500,
	-- 			custom_textobjects = {
	-- 				o = ai.gen_spec.treesitter({
	-- 					a = { "@block.outer", "@conditional.outer", "@loop.outer" },
	-- 					i = { "@block.inner", "@conditional.inner", "@loop.inner" },
	-- 				}, {}),
	-- 				f = ai.gen_spec.treesitter({ a = "@function.outer", i = "@function.inner" }, {}),
	-- 				c = ai.gen_spec.treesitter({ a = "@class.outer", i = "@class.inner" }),
	-- 			},
	-- 		})
	-- 	end,
	-- },
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
		version = false,
	},
	{
		"kylechui/nvim-surround",
		event = "VeryLazy",
		config = true,
	},
	{
		"RRethy/vim-illuminate",
		version = false,
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
		event = "BufReadPre",
		opts = {
			buftype_exclude = { "terminal", "nofile" },
			char = "│",
			show_current_context = true,
			show_current_context_start = false,
			use_treesitter = true,
			show_end_of_line = true,
		},
	},
	-- {
	-- 	"folke/noice.nvim",
	-- 	version = false,
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
	-- {
	-- 	"stevearc/dressing.nvim",
	-- 	init = function()
	-- 		---@diagnostic disable-next-line: duplicate-set-field
	-- 		vim.ui.select = function(...)
	-- 			require("lazy").load({ plugins = { "dressing.nvim" } })
	-- 			return vim.ui.select(...)
	-- 		end
	-- 		---@diagnostic disable-next-line: duplicate-set-field
	-- 		vim.ui.input = function(...)
	-- 			require("lazy").load({ plugins = { "dressing.nvim" } })
	-- 			return vim.ui.select(...)
	-- 		end
	-- 	end,
	-- },
	{
		"andymass/vim-matchup",
		event = "BufReadPost",
		version = false,
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
		"tpope/vim-unimpaired",
		event = "BufReadPost",
	},
	{
		"tpope/vim-projectionist",
		version = false,
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
  \ }}
]])
		end,
	},
	{
		-- Note taking
		"epwalsh/obsidian.nvim",
		version = false,
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
		"dstein64/vim-startuptime",
		version = false,
		cmd = "StartupTime",
		config = function()
			vim.g.startuptime_tries = 20
		end,
	},
}
