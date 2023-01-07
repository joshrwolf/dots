local M = {
	"mfussenegger/nvim-dap",
	event = "VeryLazy",
	dependencies = {
		{
			"rcarriga/nvim-dap-ui",
			config = function()
				require("dapui").setup()
			end,
		},

		"theHamsta/nvim-dap-virtual-text",
		"nvim-telescope/telescope-dap.nvim",

		-- adapters
		"leoluz/nvim-dap-go",
	},
}

function M.config()
	local dap, dapui = require("dap"), require("dapui")
	require("nvim-dap-virtual-text").setup()

	dap.defaults.fallback.terminal_win_cmd = "tabnew"

	require("dap-go").setup()

	dap.listeners.after.event_initialized["dapui_config"] = function()
		dapui.open({})
	end
	dap.listeners.before.event_terminated["dapui_config"] = function()
		dapui.close({})
	end
	dap.listeners.before.event_exited["dapui_config"] = function()
		dapui.close({})
	end

	vim.fn.sign_define("DapBreakpointRejected", { text = "ﰸ", texthl = "DiagnosticError", linehl = "", numhl = "" })
	vim.fn.sign_define("DapBreakpoint", { text = "⬤", texthl = "DiagnosticError", linehl = "", numhl = "" })
	vim.fn.sign_define("DapStopped", { text = "➔", texthl = "DiagnosticWarn", linehl = "", numhl = "" })
	vim.fn.sign_define("DapLogPoint", { text = "", texthl = "DiagnosticInfo", linehl = "", numhl = "" })
end

function M.init()
	-- These are defined individually to ensure we lazily load them only when required

	vim.keymap.set("n", "<leader>da", function()
		require("dap").toggle_breakpoint()
	end, { desc = "Toggle Breakpoint" })

	vim.keymap.set("n", "<leader>ds", function()
		require("dap").continue()
	end, { desc = "Continue" })

	vim.keymap.set("n", "<leader>dl", function()
		print("Running last...")
		require("dap").run_last()
	end, { desc = "Run Last" })

	vim.keymap.set("n", "<leader>dx", function()
		require("dap").terminate()
	end, { desc = "Exit" })

	vim.keymap.set("n", "<leader>dd", function()
		require("dapui").toggle()
	end, { desc = "Open UI" })

	vim.keymap.set("n", "<F3>", function()
		require("dap").step_into()
	end, { desc = "Step Into" })

	vim.keymap.set("n", "<F1>", function()
		require("dap").step_out()
	end, { desc = "Step Out" })

	vim.keymap.set("n", "<F2>", function()
		require("dap").step_over()
	end, { desc = "Step Over" })
end

return M
