local M = {}

function M.setup()
	require("cinnamon").setup({
		default_keymaps = true,
		default_delay = 2,
	})
end

return M
