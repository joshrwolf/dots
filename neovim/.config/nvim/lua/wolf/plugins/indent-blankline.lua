local M = {
	"lukas-reineke/indent-blankline.nvim",
	event = "BufReadPre",
}

function M.config()
	local indent = require("indent_blankline")

	indent.setup({
		buftype_exclude = { "terminal", "nofile" },
		filetype_exclude = {
			"dbout",
			"neo-tree-popup",
			"dap-repl",
			"startify",
			"dashboard",
			"log",
			"fugitive",
			"gitcommit",
			"packer",
			"vimwiki",
			"markdown",
			"txt",
			"vista",
			"help",
			"NvimTree",
			"git",
			"TelescopePrompt",
			"undotree",
			"flutterToolsOutline",
			"norg",
			"org",
			"orgagenda",
			"help",
			"Trouble",
		},
		show_current_context = true,
		show_current_context_start = true,
		use_treesitter = true,
		show_end_of_line = true,
	})
end

return M
