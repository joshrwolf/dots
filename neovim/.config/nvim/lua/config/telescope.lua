local M = {}

function M.setup()
  local actions = require "telescope.actions"
  local trouble = require "trouble.providers.telescope"
  local telescope = require "telescope"

  telescope.setup {
    defaults = {
      vimgrep_arguments = {
        "rg",
        "--color=never",
        "--no-heading",
        "--hidden",
        "--with-filename",
        "--line-number",
        "--column",
        "--smart-case",
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
        },
      },
    },
    pickers = {
      find_files = {
        hidden = true,
        file_ignore_patterns = {
          ".git/",
          ".node_modules/",
        },
      },
      live_grep = {
        hidden = true,
        file_ignore_patterns = {
          ".git/",
          ".node_modules/",
        },
      },
    },
    extensions = {
      fzf = {
        fuzzy = true,
      }
    },
  }

  telescope.load_extension "fzf"
  telescope.load_extension "file_browser"
  telescope.load_extension "frecency"

end

local previewers = require("telescope.previewers")
local builtin = require("telescope.builtin")
local conf = require("telescope.config")

local delta = previewers.new_termopen_previewer {
  get_command = function (entry)
    -- Handle status
    if entry.status == '??' or 'A ' then
      return { 'git', '-c', 'core.pager=delta', '-c', 'delta.side-by-side=false', 'diff', entry.value }
    end

    return { "git", "-c", "core.pager=delta", "-c", "delta.side-by-side=false", "diff", entry.value .. "^!" }
  end
}

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
