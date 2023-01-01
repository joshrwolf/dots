local M = {}

local function get_file()
	local file_name = vim.fn.expand("%")
	if not file_name then
		return
	end

	local extension = vim.fn.expand("%:e")
	local icon, color = require("nvim-web-devicons").get_icon_color(file_name, extension, { default = true })
	local icon_hlgroup = "FileIconColor" .. extension

	-- if not icon then
	-- 	vim.api.nvim_set_hl(0, icon_hlgroup, { fg = color })
	-- end

	return file_name

	-- return "%#" .. icon_hlgroup .. "#" .. icon .. "%*" .. " " .. file_name .. " > "
	-- return icon .. " " .. file_name .. " > "
end

function M.get_winbar()
	local navic = require("nvim-navic")

	if navic.is_available() then
		return navic.get_location()
	end
end

return M
