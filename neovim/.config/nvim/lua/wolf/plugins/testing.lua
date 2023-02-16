return {
	{
		"nvim-neotest/neotest",
		version = "*",
		dependencies = {
			"nvim-lua/plenary.nvim",
			"nvim-treesitter/nvim-treesitter",
			"nvim-neotest/neotest-plenary",
			"antoinemadec/FixCursorHold.nvim",
			"nvim-neotest/neotest-plenary",

			-- Test adapters
			"nvim-neotest/neotest-go",
			"rouge8/neotest-rust",
		},
		keys = {
			{ "<leader>tr", "<cmd>lua require('neotest').run.run()<cr>", desc = "Nearest" },
			{ "<leader>tf", "<cmd>lua require('neotest').run.run(vim.fn.expand('%'))<cr>", desc = "File" },
			{ "<leader>ta", "<cmd>lua require('neotest').run.attach()<cr>", desc = "Attach" },
			{ "<leader>tl", "<cmd>lua require('neotest').run.run_last()<cr>", desc = "Last" },
			{ "<leader>to", "<cmd>lua require('neotest').output.open({ enter = true })<cr>", desc = "Open" },
			{ "<leader>tt", "<cmd>lua require('neotest').summary.toggle()<cr>", desc = "Summary" },
			{ "[s", "<cmd>lua require('neotest').jump.prev({ status = 'failed' })<cr>", desc = "Prev failed" },
			{ "]s", "<cmd>lua require('neotest').jump.next({ status = 'failed' })<cr>", desc = "Next failed" },
		},
		config = function()
			-- get neotest namespace (api call creates or returns namespace)
			local neotest_ns = vim.api.nvim_create_namespace("neotest")
			vim.diagnostic.config({
				virtual_text = {
					format = function(diagnostic)
						local message =
							diagnostic.message:gsub("\n", " "):gsub("\t", " "):gsub("%s+", " "):gsub("^%s+", "")
						return message
					end,
				},
			}, neotest_ns)

			require("neotest").setup({
				adapters = {
					require("neotest-go")({
						experimental = {
							test_table = true,
						},
					}),
					require("neotest-plenary"),
					require("neotest-rust")({}),
				},
				-- consumers = {
				-- 	overseer = require("neotest.consumers.overseer"),
				-- },
				-- overseer = {
				-- 	enabled = true,
				-- 	force_default = true,
				-- },
			})
		end,
	},
	{
		"stevearc/overseer.nvim",
		config = true,
	},
}
