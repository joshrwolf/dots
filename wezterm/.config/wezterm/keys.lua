local wezterm = require("wezterm")
local workspace_switcher = wezterm.plugin.require("https://github.com/MLFlexer/smart_workspace_switcher.wezterm")

local act = wezterm.action
local M = {}

M.mod = "CMD"
M.ctrl = "CTRL"

-- Splits things radially
M.smart_split = wezterm.action_callback(function(window, pane)
	local dim = pane:get_dimensions()
	if dim.pixel_height > dim.pixel_width then
		window:perform_action(act.SplitVertical({ domain = "CurrentPaneDomain" }), pane)
	else
		window:perform_action(act.SplitHorizontal({ domain = "CurrentPaneDomain" }), pane)
	end
end)

---@param config Config
function M.setup(config)
	config.disable_default_key_bindings = true
	config.leader = { key = "s", mods = "CTRL", timeout_milliseconds = 1000 }

	config.keys = {
		-- Scrollback
		{ mods = M.mod, key = "k", action = act.ScrollByPage(-0.5) },
		{ mods = M.mod, key = "j", action = act.ScrollByPage(0.5) },
		-- New Tab
		{ mods = M.mod, key = "t", action = act.SpawnTab("CurrentPaneDomain") },
		-- Splits
		{ mods = M.mod, key = "d", action = act.SplitHorizontal({ domain = "CurrentPaneDomain" }) },
		{ mods = M.mod, key = "D", action = act.SplitVertical({ domain = "CurrentPaneDomain" }) },
		{ mods = M.mod, key = "n", action = M.smart_split },
		-- Tab Navigation
		{ mods = M.mod, key = "l", action = act({ ActivateTabRelative = 1 }) },
		{ mods = M.mod, key = "h", action = act({ ActivateTabRelative = -1 }) },
		{ mods = M.mod, key = "L", action = act.MoveTabRelative(1) },
		{ mods = M.mod, key = "H", action = act.MoveTabRelative(-1) },
		-- Clipboard
		{ mods = M.mod, key = "c", action = act.CopyTo("Clipboard") },
		{ mods = M.mod, key = "v", action = act.PasteFrom("Clipboard") },
		-- Pane Navigation
		M.split_nav("move", M.ctrl, "h", "Left"),
		M.split_nav("move", M.ctrl, "j", "Down"),
		M.split_nav("move", M.ctrl, "k", "Up"),
		M.split_nav("move", M.ctrl, "l", "Right"),
		-- Zooms
		{ mods = M.mod, key = "m", action = act.TogglePaneZoomState },
		{ mods = M.mod, key = "-", action = act.DecreaseFontSize },
		{ mods = M.mod, key = "+", action = act.IncreaseFontSize },
		{ mods = M.mod, key = "0", action = act.ResetFontSize },
		-- Debug
		{ mods = "LEADER", key = "d", action = act.ShowDebugOverlay },
		-- Workspaces
		{ mods = M.mod, key = "P", action = act.ActivateCommandPalette },
	}

	workspace_switcher.apply_to_config(config, "p", "CMD", function(label)
		return wezterm.format({
			{ Text = "󱂬: " .. label },
		})
	end)
end

---@param resize_or_move "resize" | "move"
---@param mods string
---@param key string
---@param dir "Right" | "Left" | "Up" | "Down"
function M.split_nav(resize_or_move, mods, key, dir)
	local event = "SplitNav_" .. resize_or_move .. "_" .. dir
	wezterm.on(event, function(win, pane)
		if M.is_nvim(pane) then
			-- pass the keys through to vim/nvim
			win:perform_action({ SendKey = { key = key, mods = mods } }, pane)
		else
			if resize_or_move == "resize" then
				win:perform_action({ AdjustPaneSize = { dir, 3 } }, pane)
			else
				win:perform_action({ ActivatePaneDirection = dir }, pane)
			end
		end
	end)
	return {
		key = key,
		mods = mods,
		action = wezterm.action.EmitEvent(event),
	}
end

function M.is_nvim(pane)
	return pane:get_user_vars().IS_NVIM == "true" or pane:get_foreground_process_name():find("n?vim")
end

return M
