local M = {}

function M.setup()
  require("nightfox").setup {
    options = {
      styles = {
      },
    },
  }

  vim.cmd[[colorscheme nordfox]]
  
end

return M
