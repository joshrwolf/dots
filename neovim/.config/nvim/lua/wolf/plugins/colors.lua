local M = {
	{
		"andersevenrud/nordic.nvim",
		lazy = false,
		enabled = false,
		priority = 999,
		config = function()
			require("nordic").colorscheme({})
		end,
	},
	{
		"EdenEast/nightfox.nvim",
		-- stuff
		enabled = true,
		lazy = false,
		priority = 999,
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
							field = "white",
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
	{
		"arcticicestudio/nord-vim",
		enabled = false,
		lazy = false,
		priority = 999,
		config = function()
			vim.cmd("colorscheme nord")
		end,
	},
	{
		"catppuccin/nvim",
		enabled = false,
		lazy = false,
		priority = 999,
		config = function()
			require("catppuccin").setup({
				flavour = "mocha",
				styles = {
					comments = { "italic" },
					functions = { "bold" },
					variables = {},
				},
				color_overrides = {
					mocha = {
						rosewater = "#8FBCBB",
						flamingo = "#D8DEE9",
						-- pink = "",
						-- mauve = "",
						red = "#BF616A",
						-- maroon = "",
						peach = "#B48EAD",
						yellow = "#8FBCBB",
						green = "#A3BE8C",
						-- teal = "",
						sky = "#81A1C1",
						-- sapphire = "",
						blue = "#88C0D0",
						lavender = "#EBCB8B",
						text = "#D8DEE9",
						subtext1 = "#ECEFF4",
						subtext0 = "#E5E9F0",
						-- overlay2 = "",
						-- overlay1 = "",
						overlay0 = "#616E88",
						-- surface2 = "",
						-- surface1 = "",
						-- surface0 = "",
						base = "#2f343f",
						mantle = "#3B4252",
						crust = "#2E3440",
					},
				},
			})
			vim.cmd("colorscheme catppuccin")
		end,
	},
}

return M
