local M = {
	"sindrets/diffview.nvim",
	cmd = { "DiffviewFileHistory", "DiffviewOpen" },
}

function M.config()
	require("diffview").setup({})
end

return M
