local vo = vim.opt_local

vo.tabstop = 4
vo.shiftwidth = 4
vo.softtabstop = 4

local wk = require("which-key")

wk.register({
	c = {
		name = "Code",
		i = { "<cmd>GoImport<cr>", "Go Import" },
		I = { "<cmd>GoImpl<cr>", "Go Implement" },
		f = { "<cmd>GoFillStruct<cr>", "Go Fill Struct" },
	},
}, { prefix = "<leader>", mode = "n", silent = true })
