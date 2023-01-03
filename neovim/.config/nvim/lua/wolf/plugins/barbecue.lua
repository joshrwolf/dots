local M = {
	"utilyre/barbecue.nvim",
	event = "VeryLazy",
	dependencies = {
		"neovim/nvim-lspconfig",
		"smiteshp/nvim-navic",
		"nvim-tree/nvim-web-devicons", -- optional dependency
	},
}

function M.config()
	require("barbecue").setup({
		attach_navic = false,
		show_modified = true,
		symbols = {
			separator = "",
		},
	})
end

return M
