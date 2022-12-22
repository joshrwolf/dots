local M = {
	"nvim-telescope/telescope.nvim",
	cmd = { "Telescope" },

	dependencies = {
		{ "nvim-lua/plenary.nvim" },
		{ "nvim-telescope/telescope-file-browser.nvim" },
		{ "nvim-telescope/telescope-symbols.nvim" },
		{ "nvim-telescope/telescope-fzf-native.nvim", build = "make" },
		{ "debugloop/telescope-undo.nvim" },
		{ "nvim-telescope/telescope-ui-select.nvim" },
	},
}

function M.config()
	local telescope = require("telescope")
	local actions = require("telescope.actions")

	telescope.setup({
		defaults = {
			mappings = {
				i = {
					["<C-j>"] = actions.move_selection_next,
					["<C-k>"] = actions.move_selection_previous,
				},
			},
		},
		extensions = {
			undo = {
				side_by_side = true,
				layout_strategy = "vertical",
				layout_config = {
					preview_height = 0.8,
				},
				mappings = {
					i = {
						["<cr>"] = require("telescope-undo.actions").yank_additions,
						["<C-cr>"] = require("telescope-undo.actions").restore,
					},
				},
			},
			["ui-select"] = {
				require("telescope.themes").get_dropdown({}),
			},
		},
	})

	telescope.load_extension("fzf")
	telescope.load_extension("ui-select")
	telescope.load_extension("file_browser")
	telescope.load_extension("undo")
end

function M.init() end

return M
