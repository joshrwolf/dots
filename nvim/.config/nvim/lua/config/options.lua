vim.opt.number = true
vim.opt.undofile = true
vim.opt.swapfile = false -- Allow opening files in multiple editors without swap prompts.
vim.opt.clipboard:append("unnamedplus") -- Use the system clipboard for ordinary yanks and puts.
vim.opt.ignorecase = true
vim.opt.smartcase = true
vim.opt.splitbelow = true
vim.opt.splitright = true
vim.opt.signcolumn = "yes"

vim.opt.confirm = true -- Offer to save modified buffers before closing them.
vim.opt.cursorline = true
vim.opt.scrolloff = 4 -- Keep context above and below the cursor.
vim.opt.splitkeep = "screen" -- Avoid scrolling existing windows when splitting.
vim.opt.jumpoptions:append("view") -- Restore the surrounding view when jumping back.

-- Show completion documentation without inserting a candidate before acceptance.
vim.opt.completeopt = { "menu", "menuone", "noselect", "popup" }
