local M = {}

function M.setup()
	require("nord").setup({
		styles = {
			comments = { italic = true },
		},
	})

	vim.cmd([[colorscheme nord]])
end

return M
