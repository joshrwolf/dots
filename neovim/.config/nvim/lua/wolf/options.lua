vim.g.mapleader = " "
vim.g.maplocalleader = " "

vim.opt.nu = true
vim.opt.relativenumber = true

vim.opt.tabstop = 4
vim.opt.softtabstop = 4
vim.opt.shiftwidth = 4
vim.opt.expandtab = true

vim.opt.wrap = false

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
vim.opt.mouse = "a"

vim.opt.scrolloff = 8
vim.opt.signcolumn = "yes"

vim.opt.hlsearch = false
vim.opt.incsearch = true

vim.opt.grepprg = "rg --vimgrep"

vim.opt.splitbelow = true
vim.opt.splitright = true

vim.o.timeoutlen = 300
vim.opt.updatetime = 50

-- Set treesitter based folds
vim.opt.foldlevel = 20
vim.opt.foldmethod = "expr"
vim.opt.foldexpr = "nvim_treesitter#foldexpr()"
