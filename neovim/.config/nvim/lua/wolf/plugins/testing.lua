return {
	"nvim-neotest/neotest",
	dependencies = {
		"nvim-lua/plenary.nvim",
		"nvim-treesitter/nvim-treesitter",
		"antoinemadec/FixCursorHold.nvim",

		-- Test adapters
		"nvim-neotest/neotest-go",
		"rouge8/neotest-rust",
	},
	keys = {
		{ "<leader>tr", "<cmd>lua require('neotest').run.run()<cr>", desc = "Run Nearest" },
		{ "<leader>tf", "<cmd>lua require('neotest').run.run(vim.fn.expand('%'))<cr>", desc = "Run File" },
		{ "<leader>tl", "<cmd>lua require('neotest').run.run_last()<cr>", desc = "Run Last" },
		{ "<leader>tt", "<cmd>lua require('neotest').summary.toggle()<cr>", desc = "Toggle Summary" },
	},
	config = function()
		require("neotest").setup({
			adapters = {
				require("neotest-go")({}),
				require("neotest-rust")({}),
			},
		})
	end,
}
