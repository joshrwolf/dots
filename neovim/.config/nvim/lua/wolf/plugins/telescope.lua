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
			git_branches = {
				layout_strategy = "vertical",
				layout_config = {
					preview_height = 0.7,
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
						["-"] = fb_actions.goto_cwd,
            -- TODO: Trying to get the git root of the current path
						-- ["="] = function(prompt_buf)
						-- 	local current_picker = require("telescope.actions.state").get_current_picker(prompt_buf)
						-- 	local finder = current_picker.finder
						-- 	local git_root_path = vim.fs.find(".git", {
						-- 		path = vim.fn.expand("%:p"),
						-- 		upward = true,
						-- 		type = "directory",
						-- 	})
						-- 	if not git_root_path then
						-- 		print("[telescope] Not in a git repository")
						-- 		return
						-- 	end
						-- 	print(vim.inspect(git_root_path))
						-- 	git_root_path = vim.fs.dirname(git_root_path[0])
						-- 	finder.cwd = git_root_path
						-- 	require("telescope._extensions.file_browser.utils").redraw_border_title(current_picker)
						-- 	current_picker:refresh(finder, { reset_prompt = true, multi = current_picker._multi })
						-- end,
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
end

function M.init() end

return M
