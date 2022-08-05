local M = {}

function M.setup()
	require("nightfox").setup({
		options = {
			transparent = true,
			terminal_colors = true,
			styles = {
				comments = "italic",
				keywords = "bold",
				types = "italic,bold",
			},
		},
	})

	vim.cmd("colorscheme nordfox")

	-- -- Changes
	-- vim.cmd("highlight CursorLine guibg=none")
	-- vim.cmd("highlight EndOfBuffer guifg=#14191e guibg=bg")
end

return M
