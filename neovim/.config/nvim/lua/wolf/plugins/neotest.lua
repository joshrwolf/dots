local M = {
	"nvim-neotest/neotest",
	dependencies = {
		"nvim-lua/plenary.nvim",
		"nvim-treesitter/nvim-treesitter",
		"antoinemadec/FixCursorHold.nvim",

		-- Test adapters
		"nvim-neotest/neotest-go",
		"rouge8/neotest-rust",
	},
}

function M.config()
	require("neotest").setup({
		adapters = {
			require("neotest-go")({}),
			require("neotest-rust")({}),
		},
	})
end

return M
