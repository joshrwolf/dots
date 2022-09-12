local M = {}

function M.setup()
  local catppuccin = require("catppuccin").setup({
    dim_inactive = {
      enabled = false,
      shade = "dark",
      percentage = 0.15,
    },
    styles = {
      comments = { "italic" },
    },
    -- compile = {
    -- 	enabled = true,
    -- 	path = vim.fn.stdpath("cache") .. "/catppuccin",
    -- },
    integrations = {
      treesitter = true,
      native_lsp = {
        enabled = true,
      },
      neotree = {
        enabled = true,
        show_root = true,
        transparent_panel = false,
      },
      neogit = true,
      fern = true,
      barbar = true,
      indent_blankline = {
        enabled = true,
        colored_indent_levels = true,
      },
      which_key = true,
      dap = {
        enabled = true,
        enable_ui = true,
      },
    },
    color_overrides = {
      mocha = {
        rosewater = "#d8dee9", --
        flamingo = "#EEBEBE",
        pink = "#b48ead", --
        -- mauve = "#CA9EE6",
        mauve = "#88C0D0",
        red = "#bf616a", --
        maroon = "#EA999C",
        peach = "#EF9F76",
        yellow = "#ebcb8b", --
        green = "#a3be8c", --
        teal = "#D8DEE9", --
        sky = "#99D1DB",
        sapphire = "#85C1DC",
        blue = "#81a1c1", --
        lavender = "#81A1C1", --

        text = "#d8dee9", --
        subtext1 = "#E5E9F0", --
        subtext0 = "#ECEFF4", --
        overlay2 = "#949cbb",
        overlay1 = "#838ba7",
        overlay0 = "#737994",
        surface2 = "#616E88", --
        surface1 = "#4C566A", --
        surface0 = "#434C5E",

        base = "#2e3440", --
        mantle = "#2e3440",
        crust = "#2e3440",
      },
    },
  })

  vim.g.catppuccin_flavour = "mocha" -- latte, frappe, macchiato, mocha
  vim.cmd([[colorscheme catppuccin]])
end

return M
