local M = {}

function M.setup()
  local catppuccin = require("catppuccin").setup {
    integrations = {
      treesitter = true,
      neotree = {
        enabled = true,
        show_root = false,
        transparent_panel = false,
      },
      which_key = true,
    },
  } 

  vim.g.catppuccin_flavour = "frappe"  -- latte, frappe, macchiato, mocha
  vim.cmd([[colorscheme catppuccin]])
end

return M
