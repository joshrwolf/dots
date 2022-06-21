local M = {}

function M.setup()
  require("nvim-treesitter.configs").setup {
    ensure_installed = "all",

    sync_install = false,

    ignore_install = {
      "phpdoc",
    },

    highlight = {
      enable = true,
      additional_vim_regex_highlighting = false,
    },

    -- context_commentstring = {},
    -- rainbow = {},
    -- autopairs = {},
    -- autotag = {},
    -- incremental_selection = {},
    -- indent = {},
  }
end

return M
