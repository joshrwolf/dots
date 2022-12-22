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
		on_attach = function(bufnr)
			local gs = require("gitsigns")
			local wk = require("which-key")

			wk.register({
				buffer = bufnr,
				["<leader>"] = {
					g = {
						p = { gs.preview_hunk, "Preview Hunk" },
						r = { gs.reset_hunk, "Reset Hunk" },
					},
				},
				["[c"] = { gs.prev_hunk, "Prev Hunk" },
				["]c"] = { gs.next_hunk, "Next Hunk" },
				["ih"] = { gs.select_hunk, "Select Inside Hunk", mode = { "o", "x" } },
				["oh"] = { gs.select_hunk, "Select Outside Hunk", mode = { "o", "x" } },
			})
		end,
	})
end

return M
