local M = {
	"petertriho/nvim-scrollbar",
	event = "BufReadPost",
}

function M.config()
	local scrollbar = require("scrollbar")

	scrollbar.setup({
		excluded_filetypes = {
			"prompt",
			"TelescopePrompt",
			"noice",
			"notify",
		},
	})
end

return M
