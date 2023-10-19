---@type Wezterm
local wezterm = require("wezterm")
---@class Config
local config = wezterm.config_builder()

wezterm.log_info("reloading")

-- require("tabs").setup(config)
require("links").setup(config)
require("keys").setup(config)

config.front_end = "WebGpu" -- Use metal rendering
-- config.webgpu_power_preference = "HighPerformance" -- Only enable with discrete GPUs

-- config.term = "wezterm"
config.window_decorations = "RESIZE"

-- Fonts
config.font_size = 13.5
config.line_height = 1.1
config.color_scheme = "Everforest Dark (Soft)"
config.font = wezterm.font({ family = "FiraCode NF" })
config.font_rules = {
	{
		intensity = "Bold",
		italic = true,
		font = wezterm.font({ family = "Maple Mono", weight = "Bold", style = "Italic" }),
	},
	{
		intensity = "Half",
		italic = true,
		font = wezterm.font({ family = "Maple Mono", weight = "DemiBold", style = "Italic" }),
	},
	{
		intensity = "Normal",
		italic = true,
		font = wezterm.font({ family = "Maple Mono", style = "Italic" }),
	},
}
config.bold_brightens_ansi_colors = "BrightAndBold"

-- config.default_cursor_style = "BlinkingBar"
config.window_padding = { left = 0, right = 0, top = 0, bottom = 0 }
config.scrollback_lines = 10000

return config --[[@as Wezterm]]
