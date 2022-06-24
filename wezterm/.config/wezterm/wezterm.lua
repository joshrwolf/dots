local wezterm = require 'wezterm';

local catppuccin = require('colors/catppuccin').setup {
  sync = true,
  sync_flavours = {
    dark = "frappe",
  },
}

return {
  font = wezterm.font("FiraCode Nerd Font"),
  font_size = 14.0,
  line_height = 1.15,
  colors = catppuccin,
  
  hide_tab_bar_if_only_one_tab = true,
  window_decorations = "RESIZE",
}
