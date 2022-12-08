-- This is mostly ripped from AstroVim:
--  https://github.com/AstroNvim/AstroNvim/blob/main/lua/core/utils/status.lua

local status = { hl = {}, init = {}, condition = {}, env = {} }

status.env.modes = {
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
	["t"] = { "TERM", "insert" },
	["nt"] = { "TERM", "insert" },
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

local function pattern_match(str, pattern_list)
	for _, pattern in ipairs(pattern_list) do
		if str:find(pattern) then
			return true
		end
	end
	return false
end

status.env.buf_matchers = {
	filetype = function(pattern_list)
		return pattern_match(vim.bo.filetype, pattern_list)
	end,
	buftype = function(pattern_list)
		return pattern_match(vim.bo.buftype, pattern_list)
	end,
	bufname = function(pattern_list)
		return pattern_match(vim.fn.fnamemodify(vim.api.nvim_buf_get_name(0), ":t"), pattern_list)
	end,
}

function status.condition.buffer_matches(patterns)
	for kind, pattern_list in pairs(patterns) do
		if status.env.buf_matchers[kind](pattern_list) then
			return true
		end
	end
	return false
end

--- A condition function if a macro is being recorded
-- @return boolean of wether or not a macro is currently being recorded
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.is_macro_recording }
function status.condition.is_macro_recording()
	return vim.fn.reg_recording() ~= ""
end

--- A condition function if search is visible
-- @return boolean of wether or not searching is currently visible
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.is_hlsearch }
function status.condition.is_hlsearch()
	return vim.v.hlsearch ~= 0
end

--- A condition function if the current file is in a git repo
-- @return boolean of wether or not the current file is in a git repo
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.is_git_repo }
function status.condition.is_git_repo()
	return vim.b.gitsigns_head or vim.b.gitsigns_status_dict
end

--- A condition function if there are any git changes
-- @return boolean of wether or not there are any git changes
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.git_changed }
function status.condition.git_changed()
	local git_status = vim.b.gitsigns_status_dict
	return git_status and (git_status.added or 0) + (git_status.removed or 0) + (git_status.changed or 0) > 0
end

--- A condition function if the current buffer is modified
-- @return boolean of wether or not the current buffer is modified
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.file_modified }
function status.condition.file_modified(bufnr)
	return vim.bo[bufnr or 0].modified
end

--- A condition function if the current buffer is read only
-- @return boolean of wether or not the current buffer is read only or not modifiable
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.file_read_only }
function status.condition.file_read_only(bufnr)
	local buffer = vim.bo[bufnr or 0]
	return not buffer.modifiable or buffer.readonly
end

--- A condition function if the current file has any diagnostics
-- @return boolean of wether or not the current file has any diagnostics
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.has_diagnostics }
function status.condition.has_diagnostics()
	return vim.g.status_diagnostics_enabled and #vim.diagnostic.get(0) > 0
end

--- A condition function if there is a defined filetype
-- @return boolean of wether or not there is a filetype
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.has_filetype }
function status.condition.has_filetype()
	return vim.fn.empty(vim.fn.expand("%:t")) ~= 1 and vim.bo.filetype and vim.bo.filetype ~= ""
end

--- A condition function if LSP is attached
-- @return boolean of wether or not LSP is attached
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.lsp_attached }
function status.condition.lsp_attached()
	return next(vim.lsp.buf_get_clients()) ~= nil
end

--- A condition function if treesitter is in use
-- @return boolean of wether or not treesitter is active
-- @usage local heirline_component = { provider = "Example Provider", condition = astronvim.status.condition.treesitter_available }
function status.condition.treesitter_available()
	return package.loaded["nvim-treesitter"] and require("nvim-treesitter.parsers").has_parser()
end

function status.component.git_diff(opts)
  
end

return status
