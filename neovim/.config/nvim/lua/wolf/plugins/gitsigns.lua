local M = {
	"lewis6991/gitsigns.nvim",
	event = "BufReadPre",
}

function M.config()
	require("gitsigns").setup({
		signs = {
			add = { hl = "GitSignsAdd", text = "▍", numhl = "GitSignsAddNr", linehl = "GitSignsAddLn" },
			change = { hl = "GitSignsChange", text = "▍", numhl = "GitSignsChangeNr", linehl = "GitSignsChangeLn" },
			delete = { hl = "GitSignsDelete", text = "▸", numhl = "GitSignsDeleteNr", linehl = "GitSignsDeleteLn" },
			topdelete = {
				hl = "GitSignsDelete",
				text = "▾",
				numhl = "GitSignsDeleteNr",
				linehl = "GitSignsDeleteLn",
			},
			changedelete = {
				hl = "GitSignsChange",
				text = "▍",
				numhl = "GitSignsChangeNr",
				linehl = "GitSignsChangeLn",
			},
			untracked = { hl = "GitSignsAdd", text = "▍", numhl = "GitSignsAddNr", linehl = "GitSignsAddLn" },
		},
		current_line_blame = true,
		current_line_blame_opts = {
			delay = 500,
		},
		on_attach = function(bufnr)
			local gs = require("gitsigns")
			local wk = require("which-key")

			-- Navigate hunks (and center after jump)
			-- NOTE: We need this fancy stuff b/c some jumps happen async
			vim.keymap.set("n", "]c", function()
				if vim.wo.diff then
					return "]czz"
				end
				vim.schedule(function()
					gs.next_hunk()
					vim.fn.feedkeys("zz")
				end)
				return "<Ignore>"
			end, { desc = "Next Hunk", expr = true })

			vim.keymap.set("n", "[c", function()
				if vim.wo.diff then
					return "[czz"
				end
				vim.schedule(function()
					gs.prev_hunk()
					vim.fn.feedkeys("zz")
				end)
				return "<Ignore>"
			end, { desc = "Prev Hunk", expr = true })

			wk.register({
				buffer = bufnr,
				["<leader>"] = {
					g = {
						p = { gs.preview_hunk, "Preview Hunk" },
						r = { gs.reset_hunk, "Reset Hunk" },
						d = { gs.diffthis, "Diff File" },
					},
				},
				["ih"] = { gs.select_hunk, "Select Inside Hunk", mode = { "o", "x" } },
				["oh"] = { gs.select_hunk, "Select Outside Hunk", mode = { "o", "x" } },
			})
		end,
	})
end

return M
