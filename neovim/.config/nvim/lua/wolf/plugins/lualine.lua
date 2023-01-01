local M = {
	"nvim-lualine/lualine.nvim",
	event = "VeryLazy",
	dependencies = {
		"kyazdani42/nvim-web-devicons",
	},
}

function M.config()
	require("lualine").setup({
		options = {
			theme = "nord",

			component_separators = "",
			section_separators = "",
		},
		sections = {
			lualine_a = { "mode" },
			lualine_b = { "branch" },
			lualine_c = {
				{ "diagnostics", sources = { "nvim_diagnostic" } },
				{ "filetype", icon_only = true, separator = "", padding = { left = 1, right = 0 } },
				{ "filename", path = 1, symbols = { modified = "  ", readonly = "", unnamed = "" } },
				{ "diff" },
			},
			lualine_x = {},
			lualine_y = { "progress" },
			lualine_z = {},
		},
		inactive_sessions = {
			lualine_a = {},
			lualine_b = {},
			lualine_c = {},
			lualine_x = {},
			lualine_y = {},
			lualine_z = {},
		},
		tabline = {},
		winbar = {
			lualine_a = {},
		},
		inactive_winbar = {},
		extensions = {
			"fugitive",
		},
	})
end

return M
