return {
	{
		"sainnhe/everforest",
		enabled = true,
		lazy = false,
		priority = 1000,
		version = false,
		config = function()
			vim.g.everforest_background = "soft"
			vim.cmd("colorscheme everforest")
		end,
	},
	{
		"sainnhe/gruvbox-material",
		enabled = true,
		lazy = false,
		priority = 1001,
		version = false,
		config = function()
			vim.g.gruvbox_material_background = "soft"
			-- vim.cmd("colorscheme gruvbox-material")
		end,
	},
	{
		"EdenEast/nightfox.nvim",
		enabled = true,
		lazy = false,
		priority = 1000,
		version = "*",
		config = function()
			require("nightfox").setup({
				options = {
					styles = {
						comments = "italic",
					},
				},
				palettes = {
					-- custom "everforest"
					nightfox = {
						bg0 = "#272e33",
						bg1 = "#2d353b",
						bg2 = "#3a464c",
						bg3 = "#434f55",
						bg4 = "#4d5960",
						fg0 = "#D3C6AA",
						fg1 = "#B9C0AB",
						-- fg2 = "",
						-- fg3 = "",
						sel0 = "#3D484D",
						sel1 = "#A7C080",
						black = "#475258",
						red = "#E67E80",
						green = "#A7C080",
						yellow = "#DBBC7F",
						blue = "#7FBBB3",
						magenta = "#D699B6",
						cyan = "#83C092",
						white = "#D3C6AA",
						orange = "#E69875",
						pink = "#D699B6",
						comment = "#859289",
					},
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
							builtin0 = "blue", -- go: "return"
							builtin1 = "white",
							builtin2 = "blue",
							-- comment = "white",
							conditional = "blue",
							const = "white",
							-- dep = "white",
							-- field = "white",
							func = "cyan",
							-- ident = "white",
							keyword = "blue",
							number = "magenta.dim",
							operator = "blue",
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
						["@property"] = { link = "@variable" }, -- changes things like pi.*Version* from matching @fields to something else
					},
				},
			})
			-- vim.cmd("colorscheme nordfox")
		end,
	},
}
