local M = {
	"sindrets/diffview.nvim",
	cmd = { "DiffviewFileHistory", "DiffviewOpen", "DiffviewToggleFiles", "DiffviewFocusFiles", "DiffviewClose" },
}

function M.config()
	require("diffview").setup({
		default_args = {
			DiffviewFileHistory = { "%" },
		},
		hooks = {
			-- Change local options in diff buffers
			diff_buf_read = function(_)
				vim.opt_local.wrap = false
				vim.opt_local.list = false
			end,
		},
		-- enhanced_diff_hl = true,
		keymaps = {
			view = { q = "<cmd>DiffviewClose<cr>" },
			file_panel = { q = "<cmd>DiffviewClose<cr>" },
			file_history_panel = { q = "<cmd>DiffviewClose<cr>" },
		},
	})
end

return M
