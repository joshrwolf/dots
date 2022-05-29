local g = vim.g 		                    -- Global variables
local opt = vim.opt 		                -- Set options (global/buffer/windows-scoped)

-- General
opt.mouse = 'a' 			                -- Enable mouse mode
opt.clipboard = 'unnamedplus' 		        -- Copy/paste to system clipboard
opt.swapfile = false 			            -- Don't use a swapfile
-- opt.completeopt = '' 	-- Autoselect options (set by nvim-cmp)

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
opt.linebreak = true                        -- Wrap on word boundary
opt.termguicolors = true                    -- Enable 24-bit RGB colors
opt.laststatus = 3                          -- Set global statusline

-- Tabs, indents
opt.expandtab = true 			            -- Spaces instead of tabs
opt.shiftwidth = 2			                -- Shift 2 spaces when tab
opt.tabstop = 2				                -- 1 tab == 2 spaces
opt.smartindent = true			            -- Autoindent new lines

