local M = {}

function M.setup()
	local actions = require("telescope.actions")
	local trouble = require("trouble.providers.telescope")
	local telescope = require("telescope")

	local tfb_actions = require("telescope").extensions.file_browser.actions

	telescope.setup({
		defaults = {
			vimgrep_arguments = {
				"rg",
				"--follow",
				"--color=never",
				"--no-heading",
				"--with-filename",
				"--line-number",
				"--column",
				"--smart-case",
				"--hidden",
				"-u",
			},
			layout_strategy = "horizontal",
			layout_config = {
				horizontal = {
					prompt_position = "bottom",
					preview_width = 0.55,
					results_width = 0.8,
				},
			},
			mappings = {
				i = {
					["<esc>"] = actions.close,
					["<C-j>"] = actions.move_selection_next,
					["<C-k>"] = actions.move_selection_previous,
					["<C-n>"] = actions.cycle_history_next,
					["<C-p>"] = actions.cycle_history_prev,
					["<C-z>"] = trouble.open_with_trouble,
					["<C-u>"] = actions.preview_scrolling_up,
					["<C-d>"] = actions.preview_scrolling_down,
					["<C-h>"] = actions.which_key,
					["<C-s>"] = actions.file_split,
					["<C-v>"] = actions.file_vsplit,
					["<C-t>"] = actions.file_tab,
				},
			},
			set_env = { ["COLORTERM"] = "truecolor" },
		},
		pickers = {
			find_files = {
				hidden = true,
				file_ignore_patterns = {
					".git/",
					"**/node_modules/**",
					"**/.terraform/**",
				},
			},
			buffers = {
				sort_lastused = true,
			},
			live_grep = {
				hidden = true,
				file_ignore_patterns = {
					".git/",
					"**/node_modules/**",
					"**/.terraform/**",
				},
			},
		},
		extensions = {
			fzf = {
				fuzzy = true,
				override_generic_sorter = true,
				override_file_sorter = true,
			},
			frecency = {
				ignore_patterns = { "*.git/*", "*/tmp/*" },
			},
			["ui-select"] = {
				require("telescope.themes").get_dropdown({}),
			},
			file_browser = {
				theme = "ivy",
				hijack_netrw = true,
				hidden = true,
				mappings = {
					["i"] = {
						["<c-n>"] = tfb_actions.create_from_prompt,
						["<c-r>"] = tfb_actions.rename,
						["<c-h>"] = tfb_actions.goto_parent_dir,
						["<c-x>"] = tfb_actions.remove,
						["<c-p>"] = tfb_actions.move,
						["<c-y>"] = tfb_actions.copy,
						["<c-a>"] = tfb_actions.select_all,
					},
				},
			},
		},
	})

	telescope.load_extension("fzf")
	telescope.load_extension("file_browser")
	telescope.load_extension("frecency")
	telescope.load_extension("ui-select")
	telescope.load_extension("live_grep_args")
	telescope.load_extension("harpoon")
	telescope.load_extension("persisted")
end

local previewers = require("telescope.previewers")
local builtin = require("telescope.builtin")
local conf = require("telescope.config")

local delta = previewers.new_termopen_previewer({
	get_command = function(entry)
		-- Handle status
		if entry.status == "??" or "A " then
			return { "git", "-c", "core.pager=delta", "-c", "delta.side-by-side=false", "diff", entry.value }
		end

		return { "git", "-c", "core.pager=delta", "-c", "delta.side-by-side=false", "diff", entry.value .. "^!" }
	end,
})

M.delta_git_commits = function(opts)
	opts = opts or {}
	opts.previewer = delta
	builtin.git_commits(opts)
end

M.delta_git_status = function(opts)
	opts = opts or {}
	opts.previewer = delta

	builtin.git_status(opts)
end

return M
