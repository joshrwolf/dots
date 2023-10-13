local wezterm = require("wezterm")

local io = require("io")
local os = require("os")
local act = wezterm.action

local function is_vim(pane)
	-- this is set by the plugin, and unset on ExitPre in Neovim
	return pane:get_user_vars().IS_NVIM == "true"
end

local function split_nav(resize_or_move, key)
	local direction_keys = {
		Left = "h",
		Down = "j",
		Up = "k",
		Right = "l",
		-- reverse lookup
		h = "Left",
		j = "Down",
		k = "Up",
		l = "Right",
	}

	return {
		key = key,
		mods = resize_or_move == "resize" and "META" or "CTRL",
		action = wezterm.action_callback(function(win, pane)
			if is_vim(pane) then
				-- pass the keys through to vim/nvim
				win:perform_action({
					SendKey = { key = key, mods = resize_or_move == "resize" and "META" or "CTRL" },
				}, pane)
			else
				if resize_or_move == "resize" then
					win:perform_action({ AdjustPaneSize = { direction_keys[key], 3 } }, pane)
				else
					win:perform_action({ ActivatePaneDirection = direction_keys[key] }, pane)
				end
			end
		end),
	}
end

wezterm.on("trigger-vim-with-visible-text", function(window, pane)
	-- Retrieve the current viewport's text.
	--
	-- Note: You could also pass an optional number of lines (eg: 2000) to
	-- retrieve that number of lines starting from the bottom of the viewport.
	local viewport_text = pane:get_lines_as_text()

	-- Create a temporary file to pass to vim
	local name = os.tmpname()
	local f = io.open(name, "w+")
	f:write(viewport_text)
	f:flush()
	f:close()

	-- Open a new window running vim and tell it to open the file
	window:perform_action(
		act.SpawnCommandInNewWindow({
			args = { "/opt/homebrew/bin/nvim", name },
		}),
		pane
	)

	-- Wait "enough" time for vim to read the file before we remove it.
	-- The window creation and process spawn are asynchronous wrt. running
	-- this script and are not awaitable, so we just pick a number.
	--
	-- Note: We don't strictly need to remove this file, but it is nice
	-- to avoid cluttering up the temporary directory.
	wezterm.sleep_ms(1000)
	os.remove(name)
end)

-- https://github.com/hurricanehrndz/cfg/blob/8993ea84ee04e2cf379d40a19851642078047e1e/dot_config/wezterm/wezterm.tmux.lua

return {
	window_padding = {
		top = 10,
		-- left = 10,
		bottom = 10,
	},
	font = wezterm.font_with_fallback({
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

		{ key = "a", mods = "CMD", action = wezterm.action.SendKey({ key = "a", mods = "ALT" }) },
		{ key = "g", mods = "CMD", action = wezterm.action.SendKey({ key = "g", mods = "ALT" }) },
		{ key = "n", mods = "CMD", action = wezterm.action.SendKey({ key = "n", mods = "ALT" }) },
		{ key = "-", mods = "CTRL", action = wezterm.action.SplitVertical({}) },
		{ key = "'", mods = "CTRL", action = wezterm.action.SplitHorizontal({}) },
		{ key = "m", mods = "CMD", action = wezterm.action.SendKey({ key = "m", mods = "ALT" }) },
		{ key = "e", mods = "CMD", action = wezterm.action.SendKey({ key = "e", mods = "ALT" }) },
		{ key = "s", mods = "CMD", action = wezterm.action.SendKey({ key = "s", mods = "ALT" }) },
		{ key = "t", mods = "CMD", action = wezterm.action.SpawnTab("CurrentPaneDomain") },
		{ key = "[", mods = "CMD", action = wezterm.action.SendKey({ key = "[", mods = "ALT" }) },
		{ key = "]", mods = "CMD", action = wezterm.action.SendKey({ key = "]", mods = "ALT" }) },

		-- { key = "h", mods = "CMD", action = wezterm.action.SendKey({ key = "h", mods = "ALT" }) },
		-- { key = "j", mods = "CMD", action = wezterm.action.SendKey({ key = "j", mods = "ALT" }) },
		{ key = "k", mods = "CMD", action = wezterm.action.ShowLauncherArgs({ flags = "FUZZY|TABS|WORKSPACES" }) },
		-- { key = "l", mods = "CMD", action = wezterm.action.SendKey({ key = "l", mods = "ALT" }) },

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

		-- move between split panes
		split_nav("move", "h"),
		split_nav("move", "j"),
		split_nav("move", "k"),
		split_nav("move", "l"),

		-- CSU Overrides
		-- ref: https://www.asciitable.com/
		{ key = "Comma", mods = "CTRL", action = wezterm.action.SendString("\x1b[44;5u") }, -- <ctrl-,>
		{ key = "Period", mods = "CTRL", action = wezterm.action.SendString("\x1b[46;5u") }, -- <ctrl-,>
		{ key = "Semicolon", mods = "CTRL", action = wezterm.action.SendString("\x1b[59;5u") }, -- <ctrl-,>
		{ key = "6", mods = "CTRL", action = wezterm.action.SendString("\x1e") }, -- <ctrl-,>
		{ key = "Backspace", mods = "CMD", action = wezterm.action.SendString("\x15") }, -- <ctrl-,>

		{ key = "e", mods = "CTRL", action = wezterm.action({ EmitEvent = "trigger-vim-with-visible-text" }) },
	},
}
