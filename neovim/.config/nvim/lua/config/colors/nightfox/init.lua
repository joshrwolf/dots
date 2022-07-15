local M = {}

function M.setup()
  require("nightfox").setup {
    options = {
      styles = {
        transparent = true,
        terminal_colors = true,
      },
    },
  }

  vim.cmd("colorscheme nordfox")

  -- Changes
  vim.cmd("highlight CursorLine guibg=none")
  vim.cmd("highlight EndOfBuffer guifg=#14191e guibg=bg")
end

return M
