local M = {}

function M.setup()
  vim.cmd([[ let g:neo_tree_remove_legacy_commands = 1 ]])

  vim.fn.sign_define("DiagnosticSignError", { text = " ", texthl = "DiagnosticSignError" })
  vim.fn.sign_define("DiagnosticSignWarn", { text = " ", texthl = "DiagnosticSignWarn" })
  vim.fn.sign_define("DiagnosticSignInfo", { text = " ", texthl = "DiagnosticSignInfo" })
  vim.fn.sign_define("DiagnosticSignHint", { text = "", texthl = "DiagnosticSignHint" })

  require("neo-tree").setup({
    close_if_last_window = true,
    close_floats_on_escape_key = true,
    enable_git_status = true,
    enable_diagnostics = false,
    log_level = 'info',
    log_to_file = false,
    source_selector = {
      winbar = true,
    },
    window = {
      position = "left",
      mapping_options = {
        noremap = true,
        nowait = true,
      },
      mappings = {
        ["h"] = "close_node",
        ["l"] = "open",
        ["H"] = "prev_source",
        ["L"] = "next_source",
        ["[c"] = "prev_git_modified",
        ["]c"] = "next_git_modified",
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
        never_show = {
          ".DS_Store",
          "thumbs.db",
        },
      },
      follow_current_file = true,
      use_libuv_file_watcher = true,
      hijack_netrw_behavior = "open_current",
    },
  })
end

return M
