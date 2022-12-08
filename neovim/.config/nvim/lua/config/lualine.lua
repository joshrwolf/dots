local M = {}

function M.setup()
	local navic = require("nvim-navic")

	local disabled = {
		"NvimTree",
		"neo-tree",
		"dashboard",
		"Outline",
		"aerial",
	}

	require("lualine").setup({
		options = {
			theme = "nord",
			globalstatus = true,
			disabled_filetypes = {
				statusline = disabled,
				winbar = disabled,
			},
		},
		winbar = {
			lualine_a = {},
			lualine_b = {},
			lualine_c = {
				{ navic.get_location, cond = navic.is_available },
			},
		},
		inactive_winbar = {
			lualine_c = {
				{ navic.get_location, cond = navic.is_available },
			},
		},
		extensions = {
			"quickfix",
			"toggleterm",
		},
	})
end

return M
