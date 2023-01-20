return {
	"nvim-telescope/telescope.nvim",
	dependencies = {
		"nvim-lua/plenary.nvim",
		{ "nvim-telescope/telescope-file-browser.nvim" },
		{ "nvim-telescope/telescope-fzf-native.nvim", build = "make" },
		{
			"nvim-telescope/telescope-live-grep-args.nvim",
			keys = {
				{ "<leader>fw", "<cmd> Telescope live_grep_args<cr>", "Find workds" },
			},
		},
		{
			"danielfalk/smart-open.nvim",
			dependencies = {
				"kkharji/sqlite.lua",
			},
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
		{ "<leader>fs", "<cmd>Telescope smart_open<cr>", desc = "Find smart" },
		{ "<leader>fk", "<cmd>Telescope keymaps<cr>", desc = "Find keymaps" },
		{ "<leader>fm", "<cmd>Telescope marks<cr>", desc = "Find marks" },
		{ "<leader>fd", "<cmd>Telescope diagnostics<cr>", desc = "Find diagnostics" },
		{ "<leader>gs", "<cmd>Telescope git_status<cr>", desc = "Git status" },
		{ "<leader>f.", "<cmd>Telescope resume<cr>", desc = "Find resume" },
		{ "\\", "<cmd>Telescope file_browser path=%:p:h<cr>", desc = "Browser" },
		{ "<c-,>", "<cmd>Telescope buffers<cr>", desc = "Find buffers", mode = { "n", "t" } },
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
					sort_mru = true,
					sort_lastused = true,
					ignore_current_buffer = true,
					mappings = {
						i = {
							["<c-x>"] = "delete_buffer",
							["<c-,>"] = actions.close,
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
							["\\"] = actions.close,
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
				smart_open = {
					show_scores = true,
				},
			},
		})

		telescope.load_extension("fzf")
		telescope.load_extension("file_browser")
		telescope.load_extension("live_grep_args")
		telescope.load_extension("dap")
		telescope.load_extension("smart_open")
	end,
}
