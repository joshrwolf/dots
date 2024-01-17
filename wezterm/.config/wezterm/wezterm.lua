---@type Wezterm
local wezterm = require("wezterm")
---@class Config
local config = wezterm.config_builder()

wezterm.log_info("reloading")

require("tabs").setup(config)
-- require("links").setup(config)
require("keys").setup(config)

config.front_end = "WebGpu" -- Use metal rendering
-- config.webgpu_power_preference = "HighPerformance" -- Only enable with discrete GPUs

-- config.term = "wezterm"
config.window_decorations = "RESIZE"

-- Fonts
config.font_size = 13
config.line_height = 1.3
config.color_scheme = "Everforest Dark (Soft)"
config.font = wezterm.font({ family = "GeistMono Nerd Font" })
config.font_rules = {
	-- {
	-- 	intensity = "Bold",
	-- 	italic = true,
	-- 	font = wezterm.font({ family = "Monospace Radon Var", weight = "Bold" }),
	-- },
	-- {
	-- 	-- intensity = "Medium",
	-- 	-- italic = true,
	-- 	font = wezterm.font({ family = "Monospace Radon Var" }),
	-- },
	-- {
	-- 	-- intensity = "Normal",
	-- 	-- italic = true,
	-- 	font = wezterm.font({ family = "Monospace Radon Var" }),
	-- },
}
config.bold_brightens_ansi_colors = "BrightAndBold"

-- config.default_cursor_style = "BlinkingBar"
config.window_padding = { left = 5, right = 0, top = 5, bottom = 0 }
config.scrollback_lines = 10000
config.enable_scroll_bar = false
config.audible_bell = "Disabled"
-- config.hide_tab_bar_if_only_one_tab = true

-- local mux = wezterm.mux
-- wezterm.on("gui-startup", function()
-- 	local _, _, window = mux.spawn_window({})
-- 	window:gui_window():maximize()
-- end)

return config --[[@as Wezterm]]
