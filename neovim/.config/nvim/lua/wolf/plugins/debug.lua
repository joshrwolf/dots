return {
	-- dap
	{
		"mfussenegger/nvim-dap",
		dependencies = {
			{
				"rcarriga/nvim-dap-ui",
				config = function()
					require("dapui").setup()
				end,
			},

			-- misc
			{ "theHamsta/nvim-dap-virtual-text", config = true },
			{
				"nvim-telescope/telescope-dap.nvim",
				keys = {
					{ "<leader>df", "<cmd>Telescope dap list_breakpoints<cr>", { desc = "Find breakpoints" } },
				},
				config = true,
			},

			-- adapters
			{
				"leoluz/nvim-dap-go",
				keys = {
					{
						"<leader>dt",
						"<cmd>lua require('dap-go').debug_test()<cr>",
						desc = "Debug nearest",
					},
				},
				config = function()
					require("dap-go").setup()
				end,
			},
		},
		keys = {
			{
				"<leader>da",
				function()
					require("dap").toggle_breakpoint()
				end,
				"Toggle Breakpoint",
			},
			{
				"<leader>ds",
				function()
					require("dap").continue()
				end,
				"Continue",
			},
			{
				"<leader>dl",
				function()
					print("Running last...")
					require("dap").run_last()
				end,
				{ desc = "Run Last" },
			},
			{
				"<leader>dx",
				function()
					require("dap").terminate()
				end,
				{ desc = "Exit" },
			},
			{
				"<leader>dd",
				function()
					require("dap").toggle()
				end,
				{ desc = "Open UI" },
			},
			{
				"<F3>",
				function()
					require("dap").step_into()
				end,
				{ desc = "Step Into" },
			},
			{
				"<F1>",
				function()
					require("dap").step_out()
				end,
				{ desc = "Step Into" },
			},
			{
				"<F2>",
				function()
					require("dap").step_over()
				end,
				{ desc = "Step Over" },
			},
			{
				"<F4>",
				function()
					require("dap").continue()
				end,
				{ desc = "Continue" },
			},
			{
				"<F5>",
				function()
					require("dap").terminate()
				end,
				{ desc = "Terminate" },
			},
		},
		config = function()
			local dap, dapui = require("dap"), require("dapui")

			dap.listeners.after.event_initialized["dapui_config"] = function()
				dapui.open({})
			end
			dap.listeners.before.event_terminated["dapui_config"] = function()
				dapui.close({})
			end
			dap.listeners.before.event_exited["dapui_config"] = function()
				dapui.close({})
			end

			vim.fn.sign_define(
				"DapBreakpointRejected",
				{ text = "ﰸ", texthl = "DiagnosticError", linehl = "", numhl = "" }
			)
			vim.fn.sign_define("DapBreakpoint", { text = "⬤", texthl = "DiagnosticError", linehl = "", numhl = "" })
			vim.fn.sign_define("DapStopped", { text = "➔", texthl = "DiagnosticWarn", linehl = "", numhl = "" })
			vim.fn.sign_define("DapLogPoint", { text = "", texthl = "DiagnosticInfo", linehl = "", numhl = "" })
		end,
	},
}
