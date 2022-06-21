local M = {}

function M.setup()
  vim.cmd([[ let g:neo_tree_remove_legacy_commands = 1 ]])

  require("neo-tree").setup {
    close_if_last_window = true,
    window = {
      position = "left",
      mapping_options = {
        noremap = true,
        nowait = true,
      },
    },
    filesystem = {
      filtered_items = {
        hide_dotfiles = false,
        hide_gitignored = false,
        hide_hidden = false,
        hide_by_name = {
          "node_modules",
          ".git",
        },
      },
      follow_current_file = true,
    },
  }
end

return M
