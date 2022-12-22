local M = {
	"andersevenrud/nordic.nvim",
	lazy = false,
}

function M.config()
	require("nordic").colorscheme({})
end

return M
