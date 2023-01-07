local M = {
	"kevinhwang91/nvim-ufo",
	event = "VeryLazy",
	dependencies = {
		"kevinhwang91/promise-async",
	},
}

-- here is a
-- long comment
-- that should get folded
function M.config()
	require("ufo").setup({
		close_fold_kinds = { "comment" },
	})
	vim.opt.foldcolumn = "1"
	vim.opt.foldlevel = 99
	vim.opt.foldlevelstart = -1
	vim.opt.foldenable = true
end

return M
