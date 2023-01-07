vim.g.mapleader = " "
vim.g.maplocalleader = " "

vim.opt.nu = true
vim.opt.relativenumber = true

vim.opt.tabstop = 4
vim.opt.softtabstop = 4
vim.opt.shiftwidth = 4
vim.opt.expandtab = true

vim.opt.wrap = false
-- Wrapped lines continue with the same indent
vim.opt.breakindent = true

-- Words from these languages are recognized when spell checking
vim.opt.spelllang = { "en" }

-- Apparently used to get rid of some redundant messages
vim.opt.shortmess = vim.opt.shortmess + "c"

vim.opt.termguicolors = true

vim.opt.autowrite = true
vim.opt.cursorline = true -- highlight current line

vim.opt.swapfile = false
vim.opt.backup = false
vim.opt.undofile = true
vim.opt.undodir = os.getenv("HOME") .. "/.vim/undodir"

vim.opt.laststatus = 3
vim.opt.showmode = false

vim.opt.hidden = true
vim.opt.ignorecase = true
vim.opt.smartcase = true
vim.opt.autoindent = true
vim.opt.mouse = "a"

vim.opt.scrolloff = 4
vim.opt.signcolumn = "yes"

vim.opt.hlsearch = true
vim.opt.incsearch = true

vim.opt.grepprg = "rg --vimgrep"

vim.opt.splitbelow = true
vim.opt.splitright = true

vim.o.timeoutlen = 300
vim.opt.updatetime = 50
