--[[

  Neovim init file
  Version: 0.0.1 - 4/23/2022
  Maintainer: joshrwolf

--]]

-- Import lua modules
require('packer_init')

-- Import core stuff
require('core.options')
require('core.autocmds')
require('core.colors')
require('core.keymaps')

-- Import plugins
require('plugins.treesitter')
require('plugins.telescope')
require('plugins.lspconfig')
require('plugins.cmp')
require('plugins.which-key')
require('plugins.lualine')
require('plugins.autopairs')
