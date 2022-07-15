local M = {}

function M.setup()
  vim.cmd([[ let g:neo_tree_remove_legacy_commands = 1 ]])

  vim.fn.sign_define("DiagnosticSignError",
    {text = " ", texthl = "DiagnosticSignError"})
  vim.fn.sign_define("DiagnosticSignWarn",
    {text = " ", texthl = "DiagnosticSignWarn"})
  vim.fn.sign_define("DiagnosticSignInfo",
    {text = " ", texthl = "DiagnosticSignInfo"})
  vim.fn.sign_define("DiagnosticSignHint",
    {text = "", texthl = "DiagnosticSignHint"})

  require("neo-tree").setup {
    close_if_last_window = true,
    enable_git_status = true,
    enable_diagnostics = true,
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
      use_libuv_file_watcher = true,
    },
  }
end

return M
