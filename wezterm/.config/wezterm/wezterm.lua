local wezterm = require("wezterm")

-- https://github.com/hurricanehrndz/cfg/blob/8993ea84ee04e2cf379d40a19851642078047e1e/dot_config/wezterm/wezterm.tmux.lua

return {
	window_padding = {
		top = 10,
		-- left = 10,
		bottom = 10,
	},
	font = wezterm.font_with_fallback({
		-- {
		-- 	family = "IBM Plex Mono",
		-- },
		{
			family = "FiraCode NF",
			harfbuzz_features = { "cv02", "ss05" },
		},
	}),
	font_rules = {
		-- -- Bold-not-italic
		-- {
		-- 	intensity = "Bold",
		-- 	italic = false,
		-- 	font = wezterm.font({
		-- 		family = "JetBrains Mono",
		-- 		weight = "Bold",
		-- 	}),
		-- },

		-- -- Bold-and-italic
		-- {
		-- 	intensity = "Bold",
		-- 	italic = true,
		-- 	font = wezterm.font({
		-- 		family = "Liga SFMono Nerd Font",
		-- 		italic = true,
		-- 	}),
		-- },
		--
		-- -- normal-intensity-and-italic
		-- {
		-- 	intensity = "Normal",
		-- 	italic = true,
		-- 	font = wezterm.font({
		-- 		family = "Liga SFMono Nerd Font",
		-- 		weight = "Regular",
		-- 		italic = true,
		-- 	}),
		-- },
		--
		-- -- half-intensity-and-italic (half-bright or dim); use a lighter weight font
		-- {
		-- 	intensity = "Half",
		-- 	italic = true,
		-- 	font = wezterm.font({
		-- 		family = "Liga SFMono Nerd Font",
		-- 		weight = "Medium",
		-- 		italic = true,
		-- 	}),
		-- },
		--
		-- -- half-intensity-and-not-italic
		-- {
		-- 	intensity = "Half",
		-- 	italic = false,
		-- 	font = wezterm.font({
		-- 		family = "FiraCode Nerd Font Mono",
		-- 		weight = "Medium",
		-- 	}),
		-- },
	},

	font_size = 13.75,
	cell_width = 1,
	line_height = 1.1,
	color_scheme = "Everforest Dark (Soft)",
	-- dpi = 144.0,

	hide_tab_bar_if_only_one_tab = true,
	window_decorations = "RESIZE",

	send_composed_key_when_left_alt_is_pressed = true,

	keys = {
		-- Get new keys with `xxd -psd`
		-- { key = "l", mods = "LEADER", action = wezterm.action.ShowLauncherArgs{ flags = "FUZZY|KEY_ASSIGNMENTS" } },
		{ key = "l", mods = "LEADER", action = wezterm.action.ShowLauncher },
		{ key = "1", mods = "CMD", action = wezterm.action.SendKey({ key = "1", mods = "ALT" }) },
		{ key = "2", mods = "CMD", action = wezterm.action.SendKey({ key = "2", mods = "ALT" }) },
		{ key = "3", mods = "CMD", action = wezterm.action.SendKey({ key = "3", mods = "ALT" }) },
		{ key = "4", mods = "CMD", action = wezterm.action.SendKey({ key = "4", mods = "ALT" }) },
		{ key = "5", mods = "CMD", action = wezterm.action.SendKey({ key = "5", mods = "ALT" }) },
		{ key = "6", mods = "CMD", action = wezterm.action.SendKey({ key = "6", mods = "ALT" }) },
		{ key = "7", mods = "CMD", action = wezterm.action.SendKey({ key = "7", mods = "ALT" }) },
		{ key = "8", mods = "CMD", action = wezterm.action.SendKey({ key = "8", mods = "ALT" }) },
		{ key = "9", mods = "CMD", action = wezterm.action.SendKey({ key = "9", mods = "ALT" }) },
		{ key = "0", mods = "CMD", action = wezterm.action.SendKey({ key = "0", mods = "ALT" }) },

		{ key = "9", mods = "CTRL", action = wezterm.action.SendString("\x14\x39") }, -- <prefix>9
		{ key = "0", mods = "CTRL", action = wezterm.action.SendString("\x14\x30") }, -- <prefix>0

		{ key = "n", mods = "CMD", action = wezterm.action.SendKey({ key = "n", mods = "ALT" }) },
		{ key = "m", mods = "CMD", action = wezterm.action.SendKey({ key = "m", mods = "ALT" }) },
		{ key = "e", mods = "CMD", action = wezterm.action.SendKey({ key = "e", mods = "ALT" }) },
		{ key = "s", mods = "CMD", action = wezterm.action.SendKey({ key = "s", mods = "ALT" }) },
		{ key = "t", mods = "CMD", action = wezterm.action.SendKey({ key = "t", mods = "ALT" }) },
		{ key = "[", mods = "CMD", action = wezterm.action.SendKey({ key = "[", mods = "ALT" }) },
		{ key = "]", mods = "CMD", action = wezterm.action.SendKey({ key = "]", mods = "ALT" }) },

		{ key = "h", mods = "CMD", action = wezterm.action.SendKey({ key = "h", mods = "ALT" }) },
		{ key = "j", mods = "CMD", action = wezterm.action.SendKey({ key = "j", mods = "ALT" }) },
		{ key = "k", mods = "CMD", action = wezterm.action.SendKey({ key = "k", mods = "ALT" }) },
		{ key = "l", mods = "CMD", action = wezterm.action.SendKey({ key = "l", mods = "ALT" }) },

		{ key = "h", mods = "SHIFT|CMD", action = wezterm.action.SendKey({ key = "LeftArrow", mods = "ALT" }) },
		{ key = "j", mods = "SHIFT|CMD", action = wezterm.action.SendKey({ key = "DownArrow", mods = "ALT" }) },
		{ key = "k", mods = "SHIFT|CMD", action = wezterm.action.SendKey({ key = "UpArrow", mods = "ALT" }) },
		{ key = "l", mods = "SHIFT|CMD", action = wezterm.action.SendKey({ key = "RightArrow", mods = "ALT" }) },

		{ key = "h", mods = "CTRL|CMD", action = wezterm.action.SendKey({ key = "y", mods = "ALT" }) },
		{ key = "j", mods = "CTRL|CMD", action = wezterm.action.SendKey({ key = "u", mods = "ALT" }) },
		{ key = "k", mods = "CTRL|CMD", action = wezterm.action.SendKey({ key = "i", mods = "ALT" }) },
		{ key = "l", mods = "CTRL|CMD", action = wezterm.action.SendKey({ key = "o", mods = "ALT" }) },

		-- Close tmux tab
		{ key = "x", mods = "CMD", action = wezterm.action.SendString("\x13\x78") }, -- <prefix>x

		-- Fuzzy find zoxide sessionizer
		{ key = "f", mods = "CMD", action = wezterm.action.SendString("\x13\x66") }, -- <prefix>f

		-- Tmux switch last session
		{ key = "z", mods = "CMD", action = wezterm.action.SendString("\x13\x7a") }, -- <prefix>z

		-- cmd p
		-- { key = "p", mods = "CMD", action = wezterm.action.SendString("\x13\x50") }, -- <prefix>p
		{ key = "p", mods = "CMD", action = wezterm.action.SendString("\x1b[44;5u") }, -- <ctrl-P>

		-- CSU Overrides
		-- ref: https://www.asciitable.com/
		{ key = "Comma", mods = "CTRL", action = wezterm.action.SendString("\x1b[44;5u") }, -- <ctrl-,>
		{ key = "Period", mods = "CTRL", action = wezterm.action.SendString("\x1b[46;5u") }, -- <ctrl-,>
		{ key = "Semicolon", mods = "CTRL", action = wezterm.action.SendString("\x1b[59;5u") }, -- <ctrl-,>
		{ key = "6", mods = "CTRL", action = wezterm.action.SendString("\x1e") }, -- <ctrl-,>
		{ key = "Backspace", mods = "CMD", action = wezterm.action.SendString("\x15") }, -- <ctrl-,>
	},
}
