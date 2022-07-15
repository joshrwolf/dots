local wezterm = require 'wezterm';
local mux = wezterm.mux;

wezterm.on("mux-startup", function ()
  local tab, pane, window = mux.spawn_window{} 
  pane:split{direction="Top"}
end)

-- https://github.com/hurricanehrndz/cfg/blob/8993ea84ee04e2cf379d40a19851642078047e1e/dot_config/wezterm/wezterm.tmux.lua

return {
  font = wezterm.font("FiraCode Nerd Font"),
  font_size = 15.0,
  cell_width = 1,
  line_height = 1.1,
  color_scheme = "nord",
  
  hide_tab_bar_if_only_one_tab = true,
  window_decorations = "RESIZE",

  send_composed_key_when_left_alt_is_pressed = true,

  -- leader = { key = "a", mods = "CTRL", timeout_milliseconds = 1000 },

  keys = {
    -- { key = "l", mods = "LEADER", action = wezterm.action.ShowLauncherArgs{ flags = "FUZZY|KEY_ASSIGNMENTS" } },
    { key = "l", mods = "LEADER", action = wezterm.action.ShowLauncher },
    { key = "1", mods = "CMD", action = wezterm.action.SendString("\x01\x31") },
    { key = "2", mods = "CMD", action = wezterm.action.SendString("\x01\x32") },
    { key = "3", mods = "CMD", action = wezterm.action.SendString("\x01\x33") },
    { key = "4", mods = "CMD", action = wezterm.action.SendString("\x01\x34") },
    { key = "9", mods = "CMD", action = wezterm.action.SendString("\x01\x39") },
    -- { key = "0", mods = "CMD", action = wezterm.action.SendString("\x01\x31\x30") },

    -- New tmux tab
    { key = "t", mods = "CMD", action = wezterm.action.SendString("\x01\x63") },

    -- Close tmux tab
    { key = "x", mods = "CMD", action = wezterm.action.SendString("\x01\x78") },
  },

  unix_domains = {
    {
      name = "test",
    }
  },
}
