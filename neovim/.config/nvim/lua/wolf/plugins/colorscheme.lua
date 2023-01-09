return {
	-- tokyonight
	{
		"folke/tokyonight.nvim",
		enabled = false,
		lazy = false,
		priority = 1000,
		config = function()
			local tokyonight = require("tokyonight")
			tokyonight.setup({
				style = "moon",
			})
			tokyonight.load()
		end,
	},
	{
		"rose-pine/neovim",
		enabled = false,
		lazy = false,
		priority = 1000,
		config = function()
			require("rose-pine").setup({
				dark_variant = "main",
			})
			vim.cmd("colorscheme rose-pine")
		end,
	},
	{
		"projekt0n/github-nvim-theme",
		enabled = false,
		lazy = false,
		priority = 1000,
		config = function()
			require("github-theme").setup({
				theme_style = "dimmed",
			})
		end,
	},
	{
		"EdenEast/nightfox.nvim",
		enabled = true,
		lazy = false,
		priority = 1000,
		config = function()
			require("nightfox").setup({
				options = {
					styles = {
						comments = "italic",
						-- keywords = "bold",
						-- functions = "bold",
					},
				},
				palettes = {
					nordfox = {
						bg0 = "#2E3440",
						bg1 = "#2E3440",
						bg2 = "#2E3440",
						bg3 = "#3B4252",
						bg4 = "#434C5E",
						-- bg4 = "#4C566A",
						-- white = "#E5E9F0",
						green = "#A3BE8C",
						cyan = "#88C0D0",
						blue = "#81A1C1",
					},
				},
				specs = {
					nordfox = {
						syntax = {
							bracket = "cyan",
							builtin0 = "blue.dim", -- go: "return"
							builtin1 = "white",
							builtin2 = "blue",
							-- comment = "white",
							conditional = "blue",
							const = "white",
							-- dep = "white",
							field = "blue.dim",
							func = "cyan",
							-- ident = "white",
							keyword = "blue",
							number = "magenta.dim",
							operator = "white.dim",
							preproc = "blue.dim", -- go: "package"
							-- regex = "white",
							statement = "white",
							-- string = "green",
							type = "cyan", -- go: "interfaces"
							variable = "white",
						},
					},
				},
				groups = {
					nordfox = {
						CmpItemKindFunction = { fg = "palette.magenta" },
						GitSignsCurrentLineBlame = { fg = "palette.fg3" },
					},
				},
			})
			vim.cmd("colorscheme nordfox")
		end,
	},
}
