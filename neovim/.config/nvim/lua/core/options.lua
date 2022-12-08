local g = vim.g -- Global variables
local o = vim.opt -- Set options (global/buffer/windows-scoped)
local map = vim.keymap.set

-- General
map({ "n", "v" }, "<Space>", "<Nop>", { noremap = true, silent = true })
g.mapleader = " "
g.maplocalleader = " "

o.cursorline = true -- Highlight current line
o.cursorlineopt = "both"
o.mouse = "a" -- Enable mouse mode
o.clipboard = "unnamed,unnamedplus" -- Copy/paste to system clipboard
o.swapfile = false -- Don't use a swapfile
o.undofile = true -- Save the undo file
o.timeoutlen = 300 -- Time in milliseconds to wait for a mapped key sequence
o.updatetime = 100

o.grepprg = "rg --hidden --vimgrep --smart-case --"

-- Neovim UI
o.number = true -- Show line numbers
o.relativenumber = true -- Relative line numbers
o.showmatch = true -- Highlight matching paranthesis
o.foldmethod = "marker" -- Enable folding (default 'foldmarker')
o.colorcolumn = "80" -- Line length marker at 80 columns
o.splitright = true -- Vertical split to the right
o.splitbelow = true -- Horizontal split to the bottom
o.ignorecase = true -- Ignore case letters when searching
o.smartcase = true -- Ignore lowercase for the whole pattern
-- opt.linebreak = true                        -- Wrap on word boundary
o.wrap = false -- Wrap
o.termguicolors = true -- Enable 24-bit RGB colors
o.hlsearch = true -- Enable highlight on search
o.laststatus = 3 -- Set global statusline

-- Tabs, indents
o.expandtab = true -- Spaces instead of tabs
o.shiftwidth = 2 -- Shift 2 spaces when tab
o.tabstop = 2 -- 1 tab == 2 spaces
o.smartindent = true -- Autoindent new lines

o.wildmode = "full"
o.wildignorecase = true
o.wildignore = [[
.git,.hg,.svn
*.aux,*.out,*.toc
*.o,*.obj,*.exe,*.dll,*.manifest,*.rbc,*.class
*.ai,*.bmp,*.gif,*.ico,*.jpg,*.jpeg,*.png,*.psd,*.webp
*.avi,*.divx,*.mp4,*.webm,*.mov,*.m2ts,*.mkv,*.vob,*.mpg,*.mpeg
*.mp3,*.oga,*.ogg,*.wav,*.flac
*.eot,*.otf,*.ttf,*.woff
*.doc,*.pdf,*.cbr,*.cbz
*.zip,*.tar.gz,*.tar.bz2,*.rar,*.tar.xz,*.kgb
*.swp,.lock,.DS_Store,._*
*/tmp/*,*.so,*.swp,*.zip,**/node_modules/**,**/target/**,**.terraform/**"
]]

o.shortmess = o.shortmess + { c = true }

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
