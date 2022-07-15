local M = {}

function M.setup()
  local catppuccin = require("catppuccin").setup {
    integrations = {
      treesitter = true,
      neotree = {
        enabled = true,
        show_root = true,
        transparent_panel = false,
      },
      indent_blankline = {
        enabled = true,
        colored_indent_levels = true,
      },
      which_key = true,
    },
  } 

  vim.g.catppuccin_flavour = "frappe"  -- latte, frappe, macchiato, mocha
  vim.cmd([[colorscheme catppuccin]])
end

return M

