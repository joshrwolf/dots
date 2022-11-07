local wezterm = require("wezterm")
local mux = wezterm.mux

wezterm.on("mux-startup", function()
	local tab, pane, window = mux.spawn_window({})
	pane:split({ direction = "Top" })
end)

-- https://github.com/hurricanehrndz/cfg/blob/8993ea84ee04e2cf379d40a19851642078047e1e/dot_config/wezterm/wezterm.tmux.lua

return {
	font = wezterm.font_with_fallback({
		-- { family = "JetBrains Mono", weight = "Medium" },
		-- { family = "MonoLisa", weight = "Regular" },
		{ family = "Fira Code", weight = "Regular" },
	}),

	font_size = 14.0,
	cell_width = 1,
	line_height = 1.05,
	color_scheme = "nord",

	hide_tab_bar_if_only_one_tab = true,
	window_decorations = "RESIZE",

	send_composed_key_when_left_alt_is_pressed = true,

	keys = {
		-- Get new keys with `xxd -psd`
		-- { key = "l", mods = "LEADER", action = wezterm.action.ShowLauncherArgs{ flags = "FUZZY|KEY_ASSIGNMENTS" } },
		{ key = "l", mods = "LEADER", action = wezterm.action.ShowLauncher },
		{ key = "1", mods = "CMD", action = wezterm.action.SendString("\x01\x31") }, -- <prefix>1
		{ key = "2", mods = "CMD", action = wezterm.action.SendString("\x01\x32") }, -- <prefix>2
		{ key = "3", mods = "CMD", action = wezterm.action.SendString("\x01\x33") }, -- <prefix>3
		{ key = "4", mods = "CMD", action = wezterm.action.SendString("\x01\x34") }, -- <prefix>4
		{ key = "9", mods = "CMD", action = wezterm.action.SendString("\x01\x39") }, -- <prefix>9

		-- New tmux tab
		{ key = "t", mods = "CMD", action = wezterm.action.SendString("\x01\x63") }, -- <prefix>t

		-- Close tmux tab
		{ key = "x", mods = "CMD", action = wezterm.action.SendString("\x01\x78") }, -- <prefix>x

		-- Fuzzy find zoxide sessionizer
		{ key = "f", mods = "CMD", action = wezterm.action.SendString("\x01\x66") }, -- <prefix>f
		{ key = "g", mods = "CMD", action = wezterm.action.SendString("\x01\x67") }, -- <prefix>g

		{ key = "m", mods = "CMD", action = wezterm.action.SendString("\x01\x6d") }, -- <prefix>g

		-- Tmux switch last session
		{ key = "z", mods = "CMD", action = wezterm.action.SendString("\x01\x7a") }, -- <prefix>z

		-- cmd p
		{ key = "p", mods = "CMD", action = wezterm.action.SendString("\x10") }, -- <prefix>p
	},

	unix_domains = {
		{
			name = "test",
		},
	},
}
