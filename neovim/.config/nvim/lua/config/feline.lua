local M = {}

local ctp_feline = require("catppuccin.groups.integrations.feline")

local lsp = require("feline.providers.lsp")
local lsp_severity = vim.diagnostic.severity
local b = vim.b

local clrs = require("catppuccin.palettes").get_palette()
local ucolors = require("catppuccin.utils.colors")
local latte = require("catppuccin.palettes.latte")

local assets = {
	left_separator = "",
	right_separator = "",
	bar = "█",
	mode_icon = "",
}

local sett = {
	text = ucolors.vary_color({ latte = latte.base }, clrs.surface0),
	bkg = ucolors.vary_color({ latte = latte.crust }, clrs.surface0),
	diffs = clrs.mauve,
	extras = clrs.overlay1,
	curr_file = clrs.maroon,
	curr_dir = clrs.flamingo,
	show_modified = false,
}

local mode_colors = {
	["n"] = { "NORMAL", clrs.lavender },
	["no"] = { "N-PENDING", clrs.lavender },
	["i"] = { "INSERT", clrs.green },
	["ic"] = { "INSERT", clrs.green },
	["t"] = { "TERMINAL", clrs.green },
	["v"] = { "VISUAL", clrs.flamingo },
	["V"] = { "V-LINE", clrs.flamingo },
	[""] = { "V-BLOCK", clrs.flamingo },
	["R"] = { "REPLACE", clrs.maroon },
	["Rv"] = { "V-REPLACE", clrs.maroon },
	["s"] = { "SELECT", clrs.maroon },
	["S"] = { "S-LINE", clrs.maroon },
	[""] = { "S-BLOCK", clrs.maroon },
	["c"] = { "COMMAND", clrs.peach },
	["cv"] = { "COMMAND", clrs.peach },
	["ce"] = { "COMMAND", clrs.peach },
	["r"] = { "PROMPT", clrs.teal },
	["rm"] = { "MORE", clrs.teal },
	["r?"] = { "CONFIRM", clrs.mauve },
	["!"] = { "SHELL", clrs.green },
}

-- Overide Components
local OC = {}

OC.file_info = {
	provider = {
		name = "file_info",
		opts = {
			type = "unique-short",
		},
	},
	hl = {
		fg = sett.text,
		bg = sett.curr_file,
	},
	left_sep = {
		str = assets.left_separator,
		hl = {
			fg = sett.curr_file,
			bg = sett.bkg,
		},
	},
}

function M.setup()
	components = {
		active = {},
		inactive = {},
	}

	table.insert(components.active, {}) -- (1) left
	table.insert(components.active, {}) -- (2) center
	table.insert(components.active, {}) -- (3) right

	-- global components
	local invi_sep = {
		str = " ",
		hl = {
			fg = sett.bkg,
			bg = sett.bkg,
		},
	}

	local vi_mode_hl = function()
		return {
			fg = sett.text,
			bg = mode_colors[vim.fn.mode()][2],
			style = "bold",
		}
	end

	-- STATUS LINE LEFT

	components.active[1][1] = {
		provider = assets.bar,
		hl = function()
			return {
				fg = mode_colors[vim.fn.mode()][2],
				bg = sett.bkg,
			}
		end,
	}

	components.active[1][2] = {
		provider = assets.mode_icon,
		hl = function()
			return {
				fg = sett.text,
				bg = mode_colors[vim.fn.mode()][2],
			}
		end,
	}

	components.active[1][3] = {
		provider = function()
			return " " .. mode_colors[vim.fn.mode()][1] .. " "
		end,
		hl = vi_mode_hl,
	}

	components.active[1][4] = {
		provider = "git_branch",
		hl = {
			fg = sett.extras,
			bg = sett.bkg,
		},
		icon = "   ",
		left_sep = invi_sep,
		right_sep = invi_sep,
	}

	components.active[1][5] = {
		provider = {
			name = "file_info",
			opts = {
				type = "unique-short",
				colored_icon = false,
			},
		},
		hl = {
			fg = sett.text,
			bg = sett.curr_file,
		},
		left_sep = invi_sep,
		right_sep = {
			str = assets.right_separator,
			hl = {
				fg = sett.curr_file,
				bg = sett.bkg,
			},
		},
	}

	-- STATUS LINE CENTER

	-- genral diagnostics (errors, warnings. info and hints)
	components.active[2][1] = {
		provider = "diagnostic_errors",
		enabled = function()
			return lsp.diagnostics_exist(lsp_severity.ERROR)
		end,

		hl = {
			fg = clrs.red,
			bg = sett.bkg,
		},
		icon = "  ",
	}

	components.active[2][2] = {
		provider = "diagnostic_warnings",
		enabled = function()
			return lsp.diagnostics_exist(lsp_severity.WARN)
		end,
		hl = {
			fg = clrs.yellow,
			bg = sett.bkg,
		},
		icon = "  ",
	}

	components.active[2][3] = {
		provider = "diagnostic_info",
		enabled = function()
			return lsp.diagnostics_exist(lsp_severity.INFO)
		end,
		hl = {
			fg = clrs.sky,
			bg = sett.bkg,
		},
		icon = "  ",
	}

	components.active[2][4] = {
		provider = "diagnostic_hints",
		enabled = function()
			return lsp.diagnostics_exist(lsp_severity.HINT)
		end,
		hl = {
			fg = clrs.rosewater,
			bg = sett.bkg,
		},
		icon = "  ",
	}

	-- STATUS LINE RIGHT
	components.active[3][1] = {
		provider = "git_diff_added",
		hl = {
			fg = sett.text,
			bg = sett.diffs,
		},
		icon = "  ",
	}

	components.active[3][2] = {
		provider = "git_diff_changed",
		hl = {
			fg = sett.text,
			bg = sett.diffs,
		},
		icon = "  ",
	}

	components.active[3][3] = {
		provider = "git_diff_removed",
		hl = {
			fg = sett.text,
			bg = sett.diffs,
		},
		icon = "  ",
	}

	require("feline").setup({
		components = components,
	})
end

return M
