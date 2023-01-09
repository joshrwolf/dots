local M = {
	"rebelot/heirline.nvim",
	event = "VimEnter",
}

local function components(opts)
	local conditions = require("heirline.conditions")
	local utils = require("heirline.utils")
	local icons = require("wolf.config.settings").icons

	local palette = require("nightfox.palette").load("nordfox")
	local colors = {
		bright_bg = utils.get_highlight("Folded").bg,
		bright_fg = utils.get_highlight("Folded").fg,
		red = utils.get_highlight("DiagnosticError").fg,
		dark_red = utils.get_highlight("DiffDelete").bg,
		green = utils.get_highlight("String").fg,
		blue = utils.get_highlight("Function").fg,
		gray = utils.get_highlight("NonText").fg,
		orange = utils.get_highlight("Constant").fg,
		purple = utils.get_highlight("Statement").fg,
		cyan = utils.get_highlight("Special").fg,
		diag_warn = utils.get_highlight("DiagnosticWarn").fg,
		diag_error = utils.get_highlight("DiagnosticError").fg,
		diag_hint = utils.get_highlight("DiagnosticHint").fg,
		diag_info = utils.get_highlight("DiagnosticInfo").fg,
		git_del = utils.get_highlight("GitSignsDelete").fg,
		git_add = utils.get_highlight("GitSignsAdd").fg,
		git_change = utils.get_highlight("GitSignsChange").fg,
	}

	local modes = {
		["n"] = { "NORMAL", "normal" },
		["no"] = { "OP", "normal" },
		["nov"] = { "OP", "normal" },
		["noV"] = { "OP", "normal" },
		["no"] = { "OP", "normal" },
		["niI"] = { "NORMAL", "normal" },
		["niR"] = { "NORMAL", "normal" },
		["niV"] = { "NORMAL", "normal" },
		["i"] = { "INSERT", "insert" },
		["ic"] = { "INSERT", "insert" },
		["ix"] = { "INSERT", "insert" },
		["t"] = { "TERM", "terminal" },
		["nt"] = { "TERM", "terminal" },
		["v"] = { "VISUAL", "visual" },
		["vs"] = { "VISUAL", "visual" },
		["V"] = { "LINES", "visual" },
		["Vs"] = { "LINES", "visual" },
		[""] = { "BLOCK", "visual" },
		["s"] = { "BLOCK", "visual" },
		["R"] = { "REPLACE", "replace" },
		["Rc"] = { "REPLACE", "replace" },
		["Rx"] = { "REPLACE", "replace" },
		["Rv"] = { "V-REPLACE", "replace" },
		["s"] = { "SELECT", "visual" },
		["S"] = { "SELECT", "visual" },
		[""] = { "BLOCK", "visual" },
		["c"] = { "COMMAND", "command" },
		["cv"] = { "COMMAND", "command" },
		["ce"] = { "COMMAND", "command" },
		["r"] = { "PROMPT", "inactive" },
		["rm"] = { "MORE", "inactive" },
		["r?"] = { "CONFIRM", "inactive" },
		["!"] = { "SHELL", "inactive" },
		["null"] = { "null", "inactive" },
	}

	return {
		Git = {
			condition = conditions.is_git_repo,
			init = function(self)
				self.status_dict = vim.b.gitsigns_status_dict
				self.has_changes = self.status_dict.added ~= 0
					or self.status_dict.removed ~= 0
					or self.status_dict.changed ~= 0
			end,
			{
				provider = function(self)
					return " " .. self.status_dict.head
				end,
				hl = { bold = true },
			},
			{
				condition = function(self)
					return self.has_changes
				end,
				provider = "(",
			},
			{
				provider = function(self)
					local count = self.status_dict.added or 0
					return count > 0 and (icons.git.added .. count .. " ")
				end,
				hl = { fg = colors.git_add },
			},
			{
				provider = function(self)
					local count = self.status_dict.removed or 0
					return count > 0 and (icons.git.removed .. count .. " ")
				end,
				hl = { fg = colors.git_del },
			},
			{
				provider = function(self)
					local count = self.status_dict.changed or 0
					return count > 0 and (icons.git.modified .. count)
				end,
				hl = { fg = colors.git_change },
			},
			{
				condition = function(self)
					return self.has_changes
				end,
				provider = ")",
			},
		},
		Diagnostics = {
			condition = conditions.has_diagnostics,
			init = function(self)
				self.errors = #vim.diagnostic.get(0, { severity = vim.diagnostic.severity.ERROR })
				self.warnings = #vim.diagnostic.get(0, { severity = vim.diagnostic.severity.WARN })
				self.hints = #vim.diagnostic.get(0, { severity = vim.diagnostic.severity.HINT })
				self.info = #vim.diagnostic.get(0, { severity = vim.diagnostic.severity.INFO })
			end,
			update = { "DiagnosticChanged", "BufEnter" },
			{
				provider = "[",
			},
			{
				provider = function(self)
					-- 0 is just another output, we can decide to print it or not!
					return self.errors > 0 and (icons.diagnostics.Error .. self.errors .. " ")
				end,
				hl = { fg = colors.diag_error },
			},
			{
				provider = function(self)
					return self.warnings > 0 and (icons.diagnostics.Warn .. self.warnings .. " ")
				end,
				hl = { fg = colors.diag_warn },
			},
			{
				provider = function(self)
					return self.info > 0 and (icons.diagnostics.Info .. self.info .. " ")
				end,
				hl = { fg = colors.diag_info },
			},
			{
				provider = function(self)
					return self.hints > 0 and (icons.diagnostics.Hint .. self.hints)
				end,
				hl = { fg = colors.diag_hint },
			},
			{
				provider = "]",
			},
		},
		File = {
			init = function(self)
				self.filename = vim.api.nvim_buf_get_name(0)
			end,
			{
				init = function(self)
					local filename = self.filename
					local extension = vim.fn.fnamemodify(filename, ":e")
					self.icon, self.icon_color =
						require("nvim-web-devicons").get_icon_color(filename, extension, { default = true })
				end,
				provider = function(self)
					return self.icon and (self.icon .. " ")
				end,
				hl = function(self)
					return { fg = self.icon_color }
				end,
			},
			{
				hl = function()
					if vim.bo.modified then
						-- use `force` because we need to override the child's hl foreground
						return { bold = true, force = true }
					end
				end,
				{
					provider = function(self)
						-- see :h filename-modifers
						local filename = vim.fn.fnamemodify(self.filename, ":t")
						if filename == "" then
							return "[No Name]"
						end
						-- now, if the filename would occupy more than 1/4th of the available
						-- space, we trim the file path to its initials
						-- See Flexible Components section below for dynamic truncation
						if not conditions.width_percent_below(#filename, 0.25) then
							filename = vim.fn.pathshorten(filename)
						end
						return filename
					end,
					hl = { fg = utils.get_highlight("Directory").fg },
				},
			},
			{
				{
					condition = function()
						return vim.bo.modified
					end,
					provider = "[+]",
					hl = { fg = palette.green.base },
				},
				{
					condition = function()
						return not vim.bo.modifiable or vim.bo.readonly
					end,
					provider = "",
					hl = { fg = "orange" },
				},
			},
		},
		Snippets = {
			condition = function()
				return vim.tbl_contains({ "s", "i" }, vim.fn.mode())
			end,
			provider = function()
				local forward = require("luasnip").jumpable(1) and "" or ""
				local backward = require("luasnip").jumpable(-1) and " " or ""
				return backward .. forward
			end,
		},
		Dap = {
			condition = function()
				local session = require("dap").session()
				return session ~= nil
			end,
			provider = function()
				return " " .. require("dap").status()
			end,
			hl = { fg = palette.orange.base },
		},
		TerminalName = {
			provider = function()
				local tname, _ = vim.api.nvim_buf_get_name(0):gsub(".*:", "")
				return " " .. tname
			end,
			hl = { fg = palette.blue.dim, bold = true },
		},
		WorkDir = {
			provider = function()
				local icon = (vim.fn.haslocaldir(0) == 1 and "l" or "g") .. " " .. " "
				local cwd = vim.fn.getcwd(0)
				cwd = vim.fn.fnamemodify(cwd, ":~")
				if not conditions.width_percent_below(#cwd, 0.25) then
					cwd = vim.fn.pathshorten(cwd)
				end
				local trail = cwd:sub(-1) == "/" and "" or "/"
				return icon .. cwd .. trail
			end,
			hl = { fg = "cyan", bold = true },
		},
		LspActive = {
			condition = conditions.lsp_attached,
			update = { "LspAttach", "LspDetach" },
			provider = function()
				local names = {}
				for i, server in pairs(vim.lsp.buf_get_clients(0)) do
					table.insert(names, server.name)
				end
				return " [" .. table.concat(names, ", ") .. "]"
			end,
			hl = { fg = palette.green.base },
		},
		Navic = {
			condition = require("nvim-navic").is_available,
			provider = function()
				return require("nvim-navic").get_location({ highlight = true })
			end,
			update = "CursorMoved",
		},
		Fill = {
			provider = "%=",
		},
		Pad = {
			provider = " ",
		},
		ScrollBar = {
			static = {
				sbar = { "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█" },
				-- Another variant, because the more choice the better.
				-- sbar = { '🭶', '🭷', '🭸', '🭹', '🭺', '🭻' }
			},
			provider = function(self)
				local curr_line = vim.api.nvim_win_get_cursor(0)[1]
				local lines = vim.api.nvim_buf_line_count(0)
				local i = math.floor((curr_line - 1) / lines * #self.sbar) + 1
				return string.rep(self.sbar[i], 2)
			end,
			hl = { fg = palette.fg2.dim, bg = palette.bg4.base },
		},
		ViMode = {
			init = function(self)
				self.mode = vim.fn.mode(1)
				-- execute this only once, this is required if you want the ViMode
				-- component to be updated on operator pending mode
				if not self.once then
					vim.api.nvim_create_autocmd("ModeChanged", {
						pattern = "*:*o",
						command = "redrawstatus",
					})
					self.once = true
				end
			end,
			static = {
				mode_colors_map = {
					normal = "cyan",
					visual = "pink",
					insert = "green",
					terminal = "orange",
					replace = "red",
					command = "yellow",
					inactive = "bg",
				},
			},
			provider = function(self)
				return "▊"
			end,
			hl = function(self)
				local generalized_mode = modes[self.mode][2]
				local fg = palette[self.mode_colors_map[generalized_mode]].base
				return { fg = fg, bold = true }
			end,
			update = { "ModeChanged" },
		},
	}
end

function M.config()
	local heirline = require("heirline")
	local conditions = require("heirline.conditions")
	local c = components()

	local default_status_line = {
		c.ViMode,
		c.Pad,
		c.Git,
		c.Pad,
		c.File,
		-- c.WorkDir,
		c.Pad,
		c.Diagnostics,
		c.Snippets,
		c.Dap,
		c.Fill,
		c.LspActive,
		c.Pad,
		c.ScrollBar,
		c.Pad,
		c.ViMode,
	}

	local terminal_status_line = {
		condition = function()
			return conditions.buffer_matches({ buftype = { "terminal" } })
		end,
		{ condition = conditions.is_active, c.ViMode },
		c.Pad,
		c.TerminalName,
		c.Fill,
	}

	heirline.setup({
		hl = function()
			if conditions.is_active() then
				return "StatusLine"
			else
				return "StatusLineNC"
			end
		end,
		fallthrough = false,

		terminal_status_line,
		default_status_line,
	}, false, false)
end

return M
