vim.opt.number = true
vim.opt.undofile = true
vim.opt.ignorecase = true
vim.opt.smartcase = true
vim.opt.splitbelow = true
vim.opt.splitright = true
vim.opt.signcolumn = "yes"

vim.opt.confirm = true -- Offer to save modified buffers before closing them.
vim.opt.cursorline = true
vim.opt.scrolloff = 4 -- Keep context above and below the cursor.
vim.opt.splitkeep = "screen" -- Avoid scrolling existing windows when splitting.

-- Show completion documentation without inserting a candidate before acceptance.
vim.opt.completeopt = { "menu", "menuone", "noselect", "popup" }

-- autoread is already enabled by default. This Neovim build also watches files
-- for external edits; modified buffers remain protected from automatic reload.
