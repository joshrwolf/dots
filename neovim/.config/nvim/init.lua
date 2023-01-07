-- TODO: Convert to this!
-- require("wolf.config.lazy")

-- Options must be set first since it sets up things like leader
require("wolf.options")
require("wolf.lazy")

-- this triggers after plugins are done loading
vim.api.nvim_create_autocmd("User", {
	pattern = "VeryLazy",
	callback = function()
		require("wolf.commands")
		require("wolf.mappings")
	end,
})
