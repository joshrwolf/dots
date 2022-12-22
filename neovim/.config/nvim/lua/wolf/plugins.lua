return {
	"folke/which-key.nvim",
	{
		"christoomey/vim-tmux-navigator",
		config = function()
			vim.g.tmux_navigator_save_on_switch = 1
		end,
		lazy = false,
	},
	{
		"luukvbaal/nnn.nvim",
		cmd = "NnnPicker",
		config = function()
			local builtin = require("nnn").builtin
			require("nnn").setup({
				picker = {
					session = "shared",
					style = { border = "rounded" },
				},
				replace_netrw = "picker",
				mappings = {
					{ "<C-v>", builtin.open_in_vsplit },
					{ "<C-p>", builtin.open_in_preview },
					{ "<C-y>", builtin.copy_to_clipboard },
				},
			})
		end,
	},
	{
		"tpope/vim-fugitive",
		lazy = false,
	},
}
