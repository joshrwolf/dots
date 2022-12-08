local M = {}

function M.setup()
	require("kanagawa").setup({
		undercurl = true,
	})

	vim.cmd([[colorscheme kanagawa]])
end

return M
