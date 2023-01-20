local wezterm = require("wezterm")

-- https://github.com/hurricanehrndz/cfg/blob/8993ea84ee04e2cf379d40a19851642078047e1e/dot_config/wezterm/wezterm.tmux.lua

return {
	font = wezterm.font_with_fallback({ { family = "Liga SFMono Nerd Font" } }),
	font_rules = {
		-- Bold-not-italic
		{
			intensity = "Bold",
			italic = false,
			font = wezterm.font({
				family = "Liga SFMono Nerd Font",
				weight = "Bold",
			}),
		},

		-- Bold-and-italic
		{
			intensity = "Bold",
			italic = true,
			font = wezterm.font({
				family = "Liga SFMono Nerd Font",
				italic = true,
			}),
		},

		-- normal-intensity-and-italic
		{
			intensity = "Normal",
			italic = true,
			font = wezterm.font({
				family = "Liga SFMono Nerd Font",
				weight = "Regular",
				italic = true,
			}),
		},

		-- half-intensity-and-italic (half-bright or dim); use a lighter weight font
		{
			intensity = "Half",
			italic = true,
			font = wezterm.font({
				family = "Liga SFMono Nerd Font",
				weight = "Medium",
				italic = true,
			}),
		},

		-- half-intensity-and-not-italic
		{
			intensity = "Half",
			italic = false,
			font = wezterm.font({
				family = "Liga SFMono Nerd Font",
				weight = "Medium",
			}),
		},
	},

	font_size = 13.0,
	cell_width = 1,
	line_height = 1.2,
	color_scheme = "nord",
	-- dpi = 144.0,

	hide_tab_bar_if_only_one_tab = true,
	window_decorations = "RESIZE",

	send_composed_key_when_left_alt_is_pressed = true,

	keys = {
		-- Get new keys with `xxd -psd`
		-- { key = "l", mods = "LEADER", action = wezterm.action.ShowLauncherArgs{ flags = "FUZZY|KEY_ASSIGNMENTS" } },
		{ key = "l", mods = "LEADER", action = wezterm.action.ShowLauncher },
		{ key = "1", mods = "CMD", action = wezterm.action.SendString("\x13\x31") }, -- <prefix>1
		{ key = "2", mods = "CMD", action = wezterm.action.SendString("\x13\x32") }, -- <prefix>2
		{ key = "4", mods = "CMD", action = wezterm.action.SendString("\x13\x33") }, -- <prefix>3
		{ key = "4", mods = "CMD", action = wezterm.action.SendString("\x13\x34") }, -- <prefix>4
		{ key = "9", mods = "CMD", action = wezterm.action.SendString("\x13\x39") }, -- <prefix>9

		-- New tmux tab
		{ key = "t", mods = "CMD", action = wezterm.action.SendString("\x13\x63") }, -- <prefix>t

		-- Close tmux tab
		{ key = "x", mods = "CMD", action = wezterm.action.SendString("\x13\x78") }, -- <prefix>x

		-- Fuzzy find zoxide sessionizer
		{ key = "f", mods = "CMD", action = wezterm.action.SendString("\x13\x66") }, -- <prefix>f
		{ key = "g", mods = "CMD", action = wezterm.action.SendString("\x13\x67") }, -- <prefix>g

		{ key = "m", mods = "CMD", action = wezterm.action.SendString("\x13\x6d") }, -- <prefix>g

		-- Tmux switch last session
		{ key = "z", mods = "CMD", action = wezterm.action.SendString("\x13\x7a") }, -- <prefix>z

		-- cmd p
		{ key = "p", mods = "CMD", action = wezterm.action.SendString("\x10") }, -- <prefix>p

		-- CSU Overrides
		{ key = "Comma", mods = "CTRL", action = wezterm.action.SendString("\x1b[44;5u") }, -- <ctrl-,>
		{ key = "Period", mods = "CTRL", action = wezterm.action.SendString("\x1b[46;5u") }, -- <ctrl-,>
		{ key = "Semicolon", mods = "CTRL", action = wezterm.action.SendString("\x1b[59;5u") }, -- <ctrl-,>
		{ key = "6", mods = "CTRL", action = wezterm.action.SendString("\x1e") }, -- <ctrl-,>
	},
}
