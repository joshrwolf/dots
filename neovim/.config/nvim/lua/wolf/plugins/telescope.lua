local M = {
	"nvim-telescope/telescope.nvim",
	cmd = { "Telescope" },

	dependencies = {
		{ "nvim-lua/plenary.nvim" },
		{ "nvim-telescope/telescope-file-browser.nvim" },
		{ "nvim-telescope/telescope-symbols.nvim" },
		{ "nvim-telescope/telescope-fzf-native.nvim", build = "make" },
		{ "debugloop/telescope-undo.nvim" },
		{ "nvim-telescope/telescope-ui-select.nvim" },
		{ "nvim-telescope/telescope-live-grep-args.nvim" },
	},
}

function M.config()
	local telescope = require("telescope")
	local actions = require("telescope.actions")
	local fb_actions = require("telescope._extensions.file_browser.actions")

	telescope.setup({
		defaults = {
			mappings = {
				n = {
					["<C-c>"] = actions.close,
				},
				i = {
					["<C-j>"] = actions.move_selection_next,
					["<C-k>"] = actions.move_selection_previous,
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
			buffers = require("telescope.themes").get_dropdown({
				sort_mru = true,
				sort_lastused = true,
				ignore_current_buffer = true,
				mappings = {
					i = { ["<c-x>"] = "delete_buffer" },
				},
				previewer = false,
			}),
			git_status = {
				layout_strategy = "vertical",
				layout_config = {
					vertical = {
						height = 0.9,
					},
				},
			},
			git_bcommits = {
				layout_strategy = "vertical",
				layout_config = {
					preview_height = 0.8,
				},
			},
			git_branches = {
				layout_strategy = "vertical",
				layout_config = {
					preview_height = 0.8,
				},
			},
		},
		extensions = {
			undo = {
				side_by_side = true,
				layout_strategy = "vertical",
				layout_config = {
					preview_height = 0.8,
				},
				mappings = {
					i = {
						["<cr>"] = require("telescope-undo.actions").yank_additions,
						["<C-cr>"] = require("telescope-undo.actions").restore,
					},
				},
			},
			["ui-select"] = {
				require("telescope.themes").get_dropdown({}),
			},
			live_grep_args = {
				mappings = {
					i = {
						["<C-k>"] = require("telescope-live-grep-args.actions").quote_prompt(),
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
				-- TODO: This doesn't work, we need to specify the entire "vimgrep_arguments" above :(
				-- additional_args = function(_)
				-- 	return { "--hidden" }
				-- end,
			},
			file_browser = {
				theme = "ivy",
				layout_config = {
					height = 50, -- make it take up 50% of the screen
				},
				hijack_netrw = true,
				hidden = true,
				hide_parent_dir = true,
				initial_mode = "normal",
				collapse_dirs = true,
				mappings = {
					-- ["i"] = {},
					["n"] = {
						["\\"] = actions.close,
						["q"] = actions.close,
						["h"] = fb_actions.goto_parent_dir,
						["l"] = actions.select_default,
						["n"] = fb_actions.create_from_prompt,
						["."] = fb_actions.toggle_hidden,
					},
				},
			},
		},
	})

	telescope.load_extension("fzf")
	telescope.load_extension("ui-select")
	telescope.load_extension("file_browser")
	telescope.load_extension("undo")
	telescope.load_extension("live_grep_args")

	--- NOTE: this must be required after setting up telescope
	--- otherwise the result will be cached without the updates
	--- from the setup call
	local builtins = require("telescope.builtin")

	local function delta_opts(opts, is_buf)
		local previewers = require("telescope.previewers")
		local delta = previewers.new_termopen_previewer({
			get_command = function(entry)
				local args = {
					"git",
					"-c",
					"core.pager=delta",
					"-c",
					"delta.side-by-side=true",
					"diff",
					entry.value .. "^!",
				}
				if is_buf then
					vim.list_extend(args, { "--", entry.current_file })
				end
				return args
			end,
		})
		opts = opts or {}
		opts.previewer = {
			delta,
			previewers.git_commit_message.new(opts),
		}
		return opts
	end

	local function delta_git_commits(opts)
		builtins.git_commits(delta_opts(opts))
	end

	local function delta_git_bcommits(opts)
		builtins.git_bcommits(delta_opts(opts, true))
	end

	local wk = require("which-key")

	wk.register({
		g = {
			s = { delta_git_bcommits, "Buffer Commits" },
			c = { delta_git_commits, "Commits" },
		},
	}, { prefix = "<leader>" })
end

function M.init() end

function M.git()
	local previewers = require("telescope.previewers")
	local builtin = require("telescope.builtin")
end

return M
