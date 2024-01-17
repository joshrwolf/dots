local wezterm = require("wezterm")

local act = wezterm.action
local M = {}

M.mod = "CMD"
M.ctrl = "CTRL"

local last_workspace = nil

local function get_zoxide_workspaces(workspace_formatter)
	local handle = io.popen('/opt/homebrew/bin/zoxide query -l | sed -e "s|^' .. wezterm.home_dir .. '/|~/|"')
	local output = handle:read("*a")
	handle:close()

	local workspace_table = {}
	for _, workspace in ipairs(wezterm.mux.get_workspace_names()) do
		table.insert(workspace_table, {
			id = workspace,
			label = workspace_formatter(workspace),
		})
	end
	for _, path in ipairs(wezterm.split_by_newlines(output)) do
		table.insert(workspace_table, {
			id = path,
			label = path,
		})
	end
	return workspace_table
end

local function workspace_switcher(workspace_formatter)
	return wezterm.action_callback(function(window, pane)
		local workspaces = get_zoxide_workspaces(workspace_formatter)

		window:perform_action(
			act.InputSelector({
				action = wezterm.action_callback(function(inner_window, inner_pane, id, label)
					if not id and not label then -- do nothing
					else
						last_workspace = window:active_workspace()
						local fullPath = string.gsub(label, "^~", wezterm.home_dir)
						if fullPath:sub(1, 1) == "/" then
							-- if path is choosen
							inner_window:perform_action(
								act.SwitchToWorkspace({
									name = label,
									spawn = {
										label = "Workspace: " .. label,
										cwd = fullPath,
									},
								}),
								inner_pane
							)
							window:set_right_status(window:active_workspace())
							-- increment path score
							wezterm.run_child_process({
								"zoxide",
								"add",
								fullPath,
							})
						else
							-- if workspace is choosen
							inner_window:perform_action(
								act.SwitchToWorkspace({
									name = id,
								}),
								inner_pane
							)
							window:set_right_status(window:active_workspace())
						end
					end
				end),
				title = "Choose Workspace",
				choices = workspaces,
				fuzzy = true,
			}),
			pane
		)
	end)
end

-- Splits things radially
M.smart_split = wezterm.action_callback(function(window, pane)
	local dim = pane:get_dimensions()
	if dim.pixel_height > dim.pixel_width then
		window:perform_action(act.SplitVertical({ domain = "CurrentPaneDomain" }), pane)
	else
		window:perform_action(act.SplitHorizontal({ domain = "CurrentPaneDomain" }), pane)
	end
end)

M.toggle_last_workspace = wezterm.action_callback(function(window, pane)
	local current_workspace = window:active_workspace()
	if last_workspace and last_workspace ~= current_workspace then
		wezterm.log_info("Switching to workspace " .. last_workspace)
		window:perform_action(act.SwitchToWorkspace({ name = last_workspace }), pane)
		last_workspace = current_workspace
	else
		last_workspace = current_workspace
	end

	-- wezterm.log_info("Last workspace: " .. last_workspace)
	-- wezterm.log_info("Current workspace: " .. current_workspace)
end)

---@param config Config
function M.setup(config)
	config.disable_default_key_bindings = true
	config.leader = { key = "s", mods = "CTRL", timeout_milliseconds = 1000 }

	config.keys = {
		-- Scrollback
		{ mods = M.mod, key = "k", action = act.ScrollByPage(-0.5) },
		{ mods = M.mod, key = "j", action = act.ScrollByPage(0.5) },
		-- { mods = M.mod, key = "f", action = act.ActivateKeyTable({ name = "search_mode" }) },
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
		{ mods = M.mod, key = "z", action = M.toggle_last_workspace },
		-- Copy pasted from https://github.com/MLFlexer/smart_workspace_switcher.wezterm/tree/main
		{
			mods = M.mod,
			key = "p",
			action = workspace_switcher(function(label)
				return wezterm.format({
					-- { Foreground = { Color = "green" } },
					{ Text = "󱂬: " .. label },
				})
			end),
		},
	}
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
				local panes = pane:tab():panes_with_info()
				local is_zoomed = false
				for _, p in ipairs(panes) do
					if p.is_zoomed then
						is_zoomed = true
					end
				end
				if is_zoomed then
					dir = dir == "Up" or dir == "Right" and "Next" or "Prev"
				end
				win:perform_action({ ActivatePaneDirection = dir }, pane)
				win:perform_action({ SetPaneZoomState = is_zoomed }, pane)
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
