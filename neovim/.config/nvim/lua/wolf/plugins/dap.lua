local M = {
	"mfussenegger/nvim-dap",

	dependencies = {
		{ "rcarriga/nvim-dap-ui" },
	},
}

function M.config()
	local dap = require("dap")
	local dapui = require("dapui")
end

return M
