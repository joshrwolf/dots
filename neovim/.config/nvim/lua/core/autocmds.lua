local autocmd = vim.api.nvim_create_autocmd -- Create autocommand

local augroup = vim.api.nvim_create_augroup -- Create autocommand group
local create_command = vim.api.nvim_create_user_command

--- Remove all trailing whitespace on save
local TrimWhiteSpaceGrp = augroup("TrimWhiteSpaceGrp", { clear = true })
autocmd("BufWritePre", {
	command = [[:%s/\s\+$//e]],
	group = TrimWhiteSpaceGrp,
})

-- go to last loc when opening a buffer
autocmd(
	"BufReadPost",
	{ command = [[if line("'\"") > 1 && line("'\"") <= line("$") | execute "normal! g`\"" | endif]] }
)

-- augroup("highlighturl", { clear = true })
-- autocmd({ "VimEnter", "FileType", "BufEnter", "WinEnter" }, {
--   desc = "URL Highlighting",
--   group = "highlighturl",
--   pattern = "*",
-- })

-- Fixed column for diagnostics to appear
-- Show autodiagnostic popup on cursor hover_range
-- Goto previous / next diagnostic warning / error
-- Show inlay_hints more frequently
vim.cmd([[
set signcolumn=yes
autocmd CursorHold * lua vim.diagnostic.open_float(nil, { focusable = false })
]])

-- Highlight on yank
augroup("YankHighlight", { clear = true })
autocmd("TextYankPost", {
	group = "YankHighlight",
	callback = function()
		vim.highlight.on_yank({ higroup = "IncSearch", timeout = "250" })
	end,
})

-- Don't auto comment new lines
autocmd("BufEnter", {
	pattern = "*",
	command = "set fo-=c fo-=r fo-=o",
})

-- Properly detect *.cue
vim.api.nvim_create_autocmd({ "BufEnter", "BufWinEnter" }, {
	pattern = { "*.cue" },
	command = "set filetype=cue",
})

-- Close nvim if NvimTree is the only remaining buffer
-- autocmd('BufEnter', {
--     command = [[if winnr('$') == 1 && bufname() == 'NvimTree_' . tabpagenr() | quit | endif ]]
-- })

-- Enter terminal in insert mode
autocmd({ "TermOpen", "BufWinEnter", "BufEnter" }, { pattern = "term://*", command = "startinsert" })

-- Autosave on lose focus, except for certain filetypes
autocmd({ "BufLeave", "FocusLost" }, {
	pattern = "*",
	callback = function()
		if string.match(vim.bo.filetype, "Neogit") then
			return
		elseif string.match(vim.bo.filetype, "Telescope") then
			return
		elseif string.match(vim.bo.filetype, "neo-tree") then
			return
		elseif string.match(vim.bo.filetype, "Trouble") then
			return
		end
		vim.cmd("silent! update")
	end,
})

-- Sync instead of :wq
-- ref: https://github.com/lukas-reineke/lsp-format.nvim#wq-will-not-format-when-not-using-sync
-- vim.cmd [[cabbrev wq execute "Format sync" <bar> wq]]
