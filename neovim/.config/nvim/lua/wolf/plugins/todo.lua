local M = {
	"folke/todo-comments.nvim",
	cmd = { "TodoTelescope" },
	event = "BufReadPost",
}

function M.config()
	require("todo-comments").setup({})
end

function M.init()
	local todo = require("todo-comments")
	vim.keymap.set("n", "]x", function()
		todo.jump_next()
	end, { desc = "Next Todo" })
	vim.keymap.set("n", "[x", function()
		todo.jump_prev()
	end, { desc = "Prev Todo" })
end

return M
