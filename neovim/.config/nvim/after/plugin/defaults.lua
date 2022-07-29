local g = vim.g 		                    -- Global variables
local opt = vim.opt 		                -- Set options (global/buffer/windows-scoped)
local map = vim.keymap.set

-- General
map({ "n", "v" }, "<Space>", "<Nop>", { noremap = true, silent = true })
g.mapleader = " "
g.maplocalleader = " "

opt.cursorline = true                 -- Highlight current line
opt.cursorlineopt = "both"
opt.mouse = 'a' 			                -- Enable mouse mode
opt.clipboard = 'unnamedplus' 		        -- Copy/paste to system clipboard
opt.swapfile = false 			            -- Don't use a swapfile
opt.undofile = true                   -- Save the undo file
opt.timeoutlen = 300                  -- Time in milliseconds to wait for a mapped key sequence
opt.updatetime = 50

-- Neovim UI
opt.number = true                           -- Show line numbers
opt.relativenumber = true                  -- Relative line numbers
opt.showmatch = true                        -- Highlight matching paranthesis
opt.foldmethod = 'marker'                   -- Enable folding (default 'foldmarker')
opt.colorcolumn = '80'                      -- Line length marker at 80 columns
opt.splitright = true                       -- Vertical split to the right
opt.splitbelow = true                       -- Horizontal split to the bottom
opt.ignorecase = true                       -- Ignore case letters when searching
opt.smartcase = true                        -- Ignore lowercase for the whole pattern
-- opt.linebreak = true                        -- Wrap on word boundary
opt.wrap = false                             -- Wrap
opt.termguicolors = true                    -- Enable 24-bit RGB colors
opt.hlsearch = true                         -- Enable highlight on search
opt.laststatus = 3                          -- Set global statusline

-- Tabs, indents
opt.expandtab = true 			            -- Spaces instead of tabs
opt.shiftwidth = 2			                -- Shift 2 spaces when tab
opt.tabstop = 2				                -- 1 tab == 2 spaces
opt.smartindent = true			            -- Autoindent new lines

opt.wildignorecase = true
opt.wildignore:append "**/node_modules/*"
opt.wildignore:append "**/.git/*"

opt.lazyredraw = true

-- Disable netrw
g.loaded_netrw = 1
g.loaded_netrwPlugin = 1

-- Neovide configs
vim.o.guifont = "Fira Code Retina"
vim.cmd([[
set guifont=FiraCode\ Nerd\ Font:h18
let g:neovide_transparency = 0.8
let g:neovide_cursor_vfx_mode = "pixiedust"
let g:neovide_cursor_animation_length = 0.05
let g:neovide_cursor_trail_length = 0.2
]])
