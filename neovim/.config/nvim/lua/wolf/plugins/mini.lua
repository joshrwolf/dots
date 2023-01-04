local M = {
	"echasnovski/mini.nvim",
	event = "VeryLazy",
	dependencies = {
		{ "JoosepAlviste/nvim-ts-context-commentstring" },
	},
}

local function pairs()
	require("mini.pairs").setup({})
end

local function comment()
	require("mini.comment").setup({
		hooks = {
			pre = function()
				require("ts_context_commentstring.internal").update_commentstring({})
			end,
		},
	})
end

local function ai()
	require("mini.ai").setup({})
end

function M.config()
	pairs()
	comment()
end

return M
