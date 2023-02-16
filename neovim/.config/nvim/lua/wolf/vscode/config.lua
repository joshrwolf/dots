vim.g.mapleader = " "
vim.g.maplocalleader = " "

vim.keymap.set(
	"n",
	"-",
	"<cmd>call VSCodeCall('workbench.files.action.showActiveFileInExplorer')<cr>",
	{ noremap = true, silent = true }
)
vim.keymap.set(
	"n",
	"]c",
	"<cmd>call VSCodeCall('workbench.action.editor.nextChange')<cr>",
	{ noremap = true, silent = true }
)
vim.keymap.set(
	"n",
	"[c",
	"<cmd>call VSCodeCall('workbench.action.editor.previousChange')<cr>",
	{ noremap = true, silent = true }
)
vim.keymap.set(
	"n",
	"<c-;>",
	"<cmd>call VSCodeCall('workbench.action.showAllEditorsByMostRecentlyUsed')<cr>",
	{ noremap = true, silent = true }
)

print("hello vscode")

return {
	{
		"wellle/targets.vim",
		event = "BufReadPost",
		verison = false,
	},
	{
		"ggandor/leap.nvim",
		event = "VeryLazy",
		config = function()
			require("leap").add_default_mappings()
		end,
	},
	{
		"andymass/vim-matchup",
		event = "BufReadPost",
		config = function()
			vim.g.matchup_matchparen_offscreen = { method = nil }
		end,
	},
}
