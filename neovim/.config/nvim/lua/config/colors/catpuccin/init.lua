local M = {}

function M.setup()
	local catppuccin = require("catppuccin").setup({
		dim_inactive = {
			enabled = true,
			shade = "dark",
			percentage = 0.15,
		},
		styles = {
			comments = { "italic" },
		},
		compile = {
			enabled = true,
			path = vim.fn.stdpath("cache") .. "/catppuccin",
		},
		integrations = {
			treesitter = true,
			native_lsp = {
				enabled = true,
			},
			neotree = {
				enabled = true,
				show_root = true,
				transparent_panel = false,
			},
			neogit = true,
			fern = true,
			barbar = true,
			indent_blankline = {
				enabled = true,
				colored_indent_levels = true,
			},
			which_key = true,
			dap = {
				enabled = true,
				enable_ui = true,
			},
		},
	})

	vim.g.catppuccin_flavour = "frappe" -- latte, frappe, macchiato, mocha
	vim.cmd([[colorscheme catppuccin]])
end

return M
