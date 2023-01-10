return {
	{
		"lewis6991/gitsigns.nvim",
		event = "BufReadPre",
		config = function()
			require("gitsigns").setup({
				signs = {
					add = { hl = "GitSignsAdd", text = "▍", numhl = "GitSignsAddNr", linehl = "GitSignsAddLn" },
					change = {
						hl = "GitSignsChange",
						text = "▍",
						numhl = "GitSignsChangeNr",
						linehl = "GitSignsChangeLn",
					},
					delete = {
						hl = "GitSignsDelete",
						text = "▸",
						numhl = "GitSignsDeleteNr",
						linehl = "GitSignsDeleteLn",
					},
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
					local gs = package.loaded.gitsigns

					local function map(mode, l, r, desc)
						vim.keymap.set(mode, l, r, { buffer = bufnr, desc = desc })
					end

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

					vim.keymap.set("n", "]c", function()
						if vim.wo.diff then
							return "]czz"
						end
						vim.schedule(function()
							gs.next_hunk()
							vim.fn.feedkeys("zz")
						end)
						return "<Ignore>"
					end, { desc = "Next Hunk", expr = true, buffer = bufnr })

					map("n", "<leader>gp", gs.preview_hunk, "Preview Hunk")
					map("n", "<leader>gr", gs.reset_hunk, "Reset Hunk")
					map("n", "<leader>gd", gs.diffthis, "Diff File")
					map("n", "<leader>gl", function()
						gs.blame_line({ full = true })
					end, "Full Blame Line")
					map({ "o", "x" }, "ih", ":<C-U>Gitsigns select_hunk<CR>", "Hunk Text Object")
					map({ "o", "x" }, "oh", ":<C-U>Gitsigns select_hunk<CR>", "Hunk Text Object")
				end,
			})
		end,
	},

	{
		"tpope/vim-fugitive",
		dependencies = {
			{
				"tpope/vim-rhubarb",
				keys = {
					{ "<leader>ghy", ":GBrowse!<cr>", "Copy permalink", mode = { "n", "v" } },
					{ "<leader>ghY", ":GBrowse! @upstream<cr>", "Copy upstream permalink", mode = { "n", "v" } },
				},
			},
		},
		cmd = { "G", "Git", "GBrowse" },
		keys = {
			{ "<leader>gg", ":tab G<cr>", "Git" },
			{ "<leader>ga", ":Gwrite<cr>", "Write file" },
			{ "<leader>gL", ":0Git log<cr>", "Git Log" },
		},
		config = function()
			vim.api.nvim_create_autocmd("FileType", {
				pattern = "fugitive",
				callback = function(ctx)
					vim.keymap.set("n", "<Tab>", "=", { remap = true, buffer = ctx.buf })
					vim.keymap.set("n", "q", ":q<cr>", { buffer = ctx.buf })
					vim.keymap.set("n", "dt", ":Gtabedit <Plug><cfile><Bar>Gdiffsplit<CR>", { buffer = ctx.buf })
					vim.keymap.set("n", "S", "<cmd>silent !git add -A<cr>", { buffer = ctx.buf, desc = "Add All" })
				end,
			})
		end,
	},

	{
		"sindrets/diffview.nvim",
		cmd = { "DiffviewFileHistory", "DiffviewOpen", "DiffviewToggleFiles", "DiffviewFocusFiles", "DiffviewClose" },
		keys = {
			{ "<leader>gf", "<cmd>DiffviewFileHistory %<cr>", "File History" },
		},
		config = function()
			require("diffview").setup({
				default_args = {
					DiffviewFileHistory = { "%" },
				},
				hooks = {
					-- Change local options in diff buffers
					diff_buf_read = function(_)
						vim.opt_local.wrap = false
						vim.opt_local.list = false
					end,
				},
				enhanced_diff_hl = true,
				keymaps = {
					view = { q = "<cmd>DiffviewClose<cr>" },
					file_panel = { q = "<cmd>DiffviewClose<cr>" },
					file_history_panel = { q = "<cmd>DiffviewClose<cr>" },
				},
			})
		end,
	},

	{
		-- ref: https://github.com/rhysd/git-messenger.vim
		"rhysd/git-messenger.vim",
		keys = {
			{ "gk", ":GitMessenger<cr>" },
		},
	},
}
