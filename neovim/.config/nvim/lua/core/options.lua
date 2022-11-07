local g = vim.g -- Global variables
local opt = vim.opt -- Set options (global/buffer/windows-scoped)
local map = vim.keymap.set

-- General
map({ "n", "v" }, "<Space>", "<Nop>", { noremap = true, silent = true })
g.mapleader = " "
g.maplocalleader = " "

opt.cursorline = true -- Highlight current line
opt.cursorlineopt = "both"
opt.mouse = "a" -- Enable mouse mode
opt.clipboard = "unnamed,unnamedplus" -- Copy/paste to system clipboard
opt.swapfile = false -- Don't use a swapfile
opt.undofile = true -- Save the undo file
opt.timeoutlen = 300 -- Time in milliseconds to wait for a mapped key sequence
opt.updatetime = 100

-- Neovim UI
opt.number = true -- Show line numbers
opt.relativenumber = true -- Relative line numbers
opt.showmatch = true -- Highlight matching paranthesis
opt.foldmethod = "marker" -- Enable folding (default 'foldmarker')
opt.colorcolumn = "80" -- Line length marker at 80 columns
opt.splitright = true -- Vertical split to the right
opt.splitbelow = true -- Horizontal split to the bottom
opt.ignorecase = true -- Ignore case letters when searching
opt.smartcase = true -- Ignore lowercase for the whole pattern
-- opt.linebreak = true                        -- Wrap on word boundary
opt.wrap = false -- Wrap
opt.termguicolors = true -- Enable 24-bit RGB colors
opt.hlsearch = true -- Enable highlight on search
opt.laststatus = 3 -- Set global statusline

-- Tabs, indents
opt.expandtab = true -- Spaces instead of tabs
opt.shiftwidth = 2 -- Shift 2 spaces when tab
opt.tabstop = 2 -- 1 tab == 2 spaces
opt.smartindent = true -- Autoindent new lines

opt.wildignorecase = true
opt.wildignore:append("**/node_modules/*")
opt.wildignore:append("**/.git/*")

opt.shortmess = opt.shortmess + { c = true }

-- Sessions: https://github.com/rmagatti/auto-session#recommended-sessionoptions-config
vim.o.sessionoptions = "blank,buffers,curdir,folds,help,tabpages,winsize,winpos,terminal"

-- Disable netrw
g.loaded_netrw = 1
g.loaded_netrwPlugin = 1

-- Neovide configs
vim.o.guifont = "Fira Code Retina"
vim.cmd([[
set guifont=FiraCode\ Nerd\ Font:h18
" let g:neovide_transparency = 1.0
let g:neovide_cursor_vfx_mode = "pixiedust"
let g:neovide_cursor_animation_length = 0.05
let g:neovide_cursor_trail_length = 0.2
]])

-- Set additional filetypes
g.do_filetype_lua = 1
vim.filetype.add({
	extension = {},
	filename = {},
	pattern = {
		["*.tfstate.*"] = "json",
	},
})

-- Highlight trailing spaces
vim.fn.matchadd("errorMsg", [[\s\+$]])
