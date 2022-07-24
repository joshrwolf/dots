local M = {}

function M.setup()
  require("nightfox").setup {
    options = {
      styles = {
        transparent = true,
        terminal_colors = true,
      },
    },

    -- ref: https://github.com/EdenEast/nightfox.nvim/blob/main/usage.md#palette
    palettes = {
      dayfox = {
        bg0 = "#E3EAF5",
        bg1 = "#D8DEE9",
        bg2 = "#C2D0E7",
        bg3 = "#B8C5DB",
        bg4 = "#AEBACF",
        fg0 = "#3B4252",
        fg1 = "#2d2d2d",
        -- fg2 = "",
        -- fg3 = "",
        sel0 = "#888888",
        -- sel1 = "",
        comment = "#2C7088",
        black = "#4C566A",
        red = "#99324B",
        green = "#4F894C",
        yellow = "#9A7500",
        blue = "#3B6EA8",
        magenta = "#97365B",
        cyan = "#398EAC",
        -- white = "#bfbfbf",
        orange = "#AC4426",
        pink = "#842879",
      }
    }
  }

  vim.o.background = "dark"
  vim.cmd("colorscheme nordfox")

  -- -- Changes
  -- vim.cmd("highlight CursorLine guibg=none")
  -- vim.cmd("highlight EndOfBuffer guifg=#14191e guibg=bg")
end

return M
