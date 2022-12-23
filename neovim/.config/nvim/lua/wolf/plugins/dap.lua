local M = {
	"mfussenegger/nvim-dap",

	dependencies = {
		{ "rcarriga/nvim-dap-ui" },
	},
}

function M.config()
	local dap = require("dap")
	local dapui = require("dapui")

	dapui.setup()

	vim.fn.sign_define("DapBreakpointRejected", { text = "ﰸ", texthl = "DiagnosticError", linehl = "", numhl = "" })
	vim.fn.sign_define("DapBreakpoint", { text = "⬤", texthl = "DiagnosticError", linehl = "", numhl = "" })
	vim.fn.sign_define("DapStopped", { text = "➔", texthl = "DiagnosticWarn", linehl = "", numhl = "" })
	vim.fn.sign_define("DapLogPoint", { text = "", texthl = "DiagnosticInfo", linehl = "", numhl = "" })
end

function M.init()
	local dap = require("dap")
	local wk = require("which-key")

	wk.register({
		d = {
			name = "+debug",
			a = { dap.toggle_breakpoint, "Toggle Breakpoint" },
			c = { dap.continue, "Continue" },
		},
	}, { prefix = "<leader>" })
end

return M
