local M = {
	"lukas-reineke/indent-blankline.nvim",
	event = "BufReadPre",
}

function M.config()
	local indent = require("indent_blankline")

	indent.setup({
		buftype_exclude = { "terminal", "nofile" },
		filetype_exclude = {
			"help",
			"startify",
			"dashboard",
			"packer",
			"neogitstatus",
			"NvimTree",
			"neo-tree",
			"Trouble",
		},
		show_current_context = true,
		show_current_context_start = false,
		use_treesitter = true,
		show_end_of_line = true,
	})
end

return M
