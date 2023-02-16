local git_delta_status = function(opts)
	local previewers = require("telescope.previewers")

	local delta = previewers.new_termopen_previewer({
		get_command = function(entry)
			-- this is for status
			-- You can get the AM things in entry.status. So we are displaying file if entry.status == '??' or 'A '
			-- just do an if and return a different command
			if entry.status == "??" or "A " then
				return { "git", "-c", "core.pager=delta", "-c", "delta.side-by-side=false", "diff", entry.path }
			end

			-- note we can't use pipes
			-- this command is for git_commits and git_bcommits
			return { "git", "-c", "core.pager=delta", "-c", "delta.side-by-side=false", "diff", entry.path .. "^!" }
		end,
	})

	opts = opts or {}
	opts.previewer = delta

	local builtin = require("telescope.builtin")
	builtin.git_status(opts)
end

return {

	{
		"nvim-telescope/telescope.nvim",
		dependencies = {
			"nvim-lua/plenary.nvim",
			{ "nvim-telescope/telescope-file-browser.nvim" },
			{ "nvim-telescope/telescope-fzf-native.nvim", build = "make" },
			{
				"stevearc/aerial.nvim",
				keys = {
					-- NOTE: Aerial doesn't respect lua table config for whatever reason, so we have to inline it in the call below
					{
						"<leader>fs",
						-- "<cmd>lua require('telescope').extensions.aerial.aerial(require('telescope.themes').get_dropdown({layout_config={width=.9, height=.8}}))<cr>",
						"<cmd>lua require('telescope').extensions.aerial.aerial({layout_strategy='vertical',layout_config={preview_height=0.6}})<cr>",
						desc = "Code Outline",
					},
				},
				config = true,
			},
		},
		cmd = "Telescope",
		keys = {
			{ "<leader><leader>", "<cmd>Telescope find_files<cr>", desc = "Find files" },
			{ "<leader>ff", "<cmd>Telescope find_files<cr>", desc = "Find files" },
			{ "<leader>gb", "<cmd>Telescope git_branches<cr>", "Git branches" },
			{ "<leader>fb", "<cmd>Telescope buffers<cr>", desc = "Find buffers" },
			{ "<leader>fo", "<cmd>Telescope oldfiles<cr>", desc = "Find oldfiles" },
			{ "<leader>fg", "<cmd>Telescope git_status<cr>", desc = "Find git status" },
			{ "<leader>fk", "<cmd>Telescope keymaps<cr>", desc = "Find keymaps" },
			{ "<leader>fm", "<cmd>Telescope marks<cr>", desc = "Find marks" },
			{ "<leader>fd", "<cmd>Telescope diagnostics<cr>", desc = "Find diagnostics" },
			{ "<leader>gs", git_delta_status, desc = "Git status" },
			{ "<leader>f.", "<cmd>Telescope resume<cr>", desc = "Find resume" },
			{ "<leader>f/", "<cmd>Telescope current_buffer_fuzzy_find<cr>", desc = "Find current buffer" },
			{ ";", "<cmd>Telescope file_browser path=%:p:h<cr>", desc = "Browser" },
			{ "<c-;>", "<cmd>Telescope buffers<cr>", desc = "Find buffers", mode = { "n", "t" } },
			{ "<leader>hh", "<cmd>Telescope help_tags<cr>", desc = "Help pages" },
			{ "<leader>hc", "<cmd>Telescope commands<cr>", desc = "Help commands" },
			{ "<leader>ha", "<cmd>Telescope autocommands<cr>", desc = "Help autocommands" },
		},
		config = function()
			local telescope = require("telescope")
			local actions = require("telescope.actions")
			local fb_actions = require("telescope._extensions.file_browser.actions")

			telescope.setup({
				defaults = {
					prompt_prefix = " ",
					selection_caret = " ",
					mappings = {
						n = {
							["<cr>"] = actions.select_default + actions.center,
							["<c-x>"] = actions.select_horizontal + actions.center,
							["<c-v>"] = actions.select_vertical + actions.center,
							["<c-t>"] = actions.select_tab + actions.center,
							["<c-c>"] = actions.close,
							["q"] = actions.close,
							["gg"] = actions.move_to_top,
							["G"] = actions.move_to_bottom,
						},
						i = {
							["<cr>"] = actions.select_default + actions.center,
							["<c-x>"] = actions.select_horizontal + actions.center,
							["<c-v>"] = actions.select_vertical + actions.center,
							["<c-t>"] = actions.select_tab + actions.center,
							["<c-j>"] = actions.move_selection_next,
							["<c-k>"] = actions.move_selection_previous,
						},
					},
					file_ignore_patterns = {
						"^node_modules/",
						"third_party/",
						"^.git/",
						"^.intellij/",
						"vendor",
						"packer_compiled",
						"%.DS_Store",
						"%.ttf",
						"%.png",
						"^site-packages/",
						"^.yarn/",
					},
				},
				pickers = {
					find_files = {
						hidden = true,
					},
					buffers = require("telescope.themes").get_ivy({
						layout_config = {
							height = 50,
						},
						sort_mru = true,
						sort_lastused = true,
						ignore_current_buffer = true,
						mappings = {
							i = {
								["<c-x>"] = "delete_buffer",
								["<c-;>"] = actions.close,
							},
						},
					}),
					git_branches = require("telescope.themes").get_dropdown({
						layout_config = {
							width = 150,
						},
					}),
					lsp_references = require("telescope.themes").get_ivy({
						layout_config = { height = 0.5 },
						include_declaration = false,
						trim_text = true,
						-- show_line = false,
						fname_width = 80,
					}),
				},
				extensions = {
					file_browser = {
						theme = "ivy",
						layout_config = {
							height = 50, -- make it take up 50% of the screen
						},
						hijack_netrw = true,
						hidden = true,
						hide_parent_dir = true,
						initial_mode = "normal",
						collapse_dirs = false,
						mappings = {
							["i"] = {
								["<cr>"] = actions.select_default,
								["<c-x>"] = actions.select_horizontal,
								["<c-v>"] = actions.select_vertical,
								["<c-t>"] = actions.select_tab,
							},
							["n"] = {
								["<cr>"] = actions.select_default,
								["<c-x>"] = actions.select_horizontal,
								["<c-v>"] = actions.select_vertical,
								["<c-t>"] = actions.select_tab,
								[";"] = actions.close,
								["q"] = actions.close,
								["h"] = fb_actions.goto_parent_dir,
								["l"] = actions.select_default,
								["n"] = fb_actions.create_from_prompt,
								["."] = fb_actions.toggle_hidden,
								["-"] = fb_actions.goto_cwd,
							},
						},
					},
					live_grep_args = {
						mappings = {
							i = {
								["<c-l>"] = require("telescope-live-grep-args.actions").quote_prompt(),
							},
						},
						vimgrep_arguments = {
							"rg",
							"--color=never",
							"--no-heading",
							"--with-filename",
							"--line-number",
							"--column",
							"--smart-case",
							"--hidden",
						},
					},
					aerial = {
						show_nesting = {
							["_"] = true,
						},
					},
				},
			})

			telescope.load_extension("fzf")
			telescope.load_extension("file_browser")
			telescope.load_extension("live_grep_args")
			telescope.load_extension("dap")
			telescope.load_extension("aerial")
		end,
	},
	{
		"nvim-telescope/telescope-live-grep-args.nvim",
		keys = {
			{ "<leader>fw", "<cmd>Telescope live_grep_args<cr>", "Find words" },
		},
		config = function()
			require("telescope").load_extension("live_grep_args")
		end,
	},
}
