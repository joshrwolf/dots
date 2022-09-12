local M = {}

function M.setup()
	local neogit = require("neogit")

	neogit.setup({
		disable_commit_confirmation = true,
		disable_insert_on_commit = false,
		integrations = {
			diffview = true,
		},
		popup = {
			kind = "split",
		},
		commit_popup = {
			kind = "split",
		},
	})
end

return M
