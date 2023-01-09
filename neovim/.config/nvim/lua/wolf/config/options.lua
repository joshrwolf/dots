vim.g.mapleader = " "
vim.g.maplocalleader = " "

vim.opt.clipboard = "unnamedplus" -- sync with system clipboard
vim.opt.cmdheight = 1
vim.opt.mouse = "a"
vim.opt.confirm = true -- confirm certain failed operations (like saving an unsaved buffer on exit)
vim.opt.number = true -- show line number
vim.opt.rnu = true -- relative numbers
vim.opt.ignorecase = true -- ignore case in searches
vim.opt.smartindent = true
vim.opt.hidden = true
vim.opt.showmode = false -- use status line
vim.opt.cursorline = true
vim.opt.expandtab = true -- use spaces instead of tab
vim.opt.shiftwidth = 2 -- size of an indent
vim.opt.tabstop = 2
vim.opt.softtabstop = 2
vim.opt.laststatus = 3
vim.opt.list = false -- show whitespace chars
vim.opt.scrolloff = 4
vim.opt.sidescrolloff = 8
vim.opt.sessionoptions = { "buffers", "curdir", "tabpages", "winsize" }
vim.opt.pumblend = 10 -- popup transparency
vim.opt.pumheight = 20 -- maximum number of items in a popup
vim.opt.grepformat = "%f:%l:%c:%m"
vim.opt.grepprg = "rg --vimgrep"
vim.opt.signcolumn = "yes" -- always show sign column
vim.opt.spelllang = { "en" }
vim.opt.splitbelow = true -- put new windows below current
vim.opt.splitright = true -- put new windows right of current
vim.opt.termguicolors = true
vim.opt.timeoutlen = 400 -- len before mapping times out
vim.opt.swapfile = false
vim.opt.backup = false
vim.opt.undofile = true
vim.opt.undolevels = 10000
vim.opt.undodir = os.getenv("HOME") .. "/.vim/undodir"
vim.opt.updatetime = 400 -- save swap file and trigger CursorHold
vim.opt.wildmode = "longest:full,full"
vim.go.winminwidth = 5
vim.opt.wrap = false
vim.opt.breakindent = true -- wrapped lines continue with the same indent
vim.lsp.set_log_level("error")
