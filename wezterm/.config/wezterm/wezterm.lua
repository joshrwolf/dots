local wezterm = require 'wezterm';
local mux = wezterm.mux;

wezterm.on("mux-startup", function ()
  local tab, pane, window = mux.spawn_window{} 
  pane:split{direction="Top"}
end)

-- https://github.com/hurricanehrndz/cfg/blob/8993ea84ee04e2cf379d40a19851642078047e1e/dot_config/wezterm/wezterm.tmux.lua

return {
  font = wezterm.font("FiraCode Nerd Font"),
  font_size = 14.0,
  cell_width = 1,
  line_height = 1.15,
  color_scheme = "nord",
  
  hide_tab_bar_if_only_one_tab = true,
  window_decorations = "RESIZE",

  -- leader = { key = "a", mods = "CTRL", timeout_milliseconds = 1000 },

  keys = {
    -- { key = "l", mods = "LEADER", action = wezterm.action.ShowLauncherArgs{ flags = "FUZZY|KEY_ASSIGNMENTS" } },
    { key = "l", mods = "LEADER", action = wezterm.action.ShowLauncher },
  },

  unix_domains = {
    {
      name = "test",
    }
  },
}
