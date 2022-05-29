-- Automagically install packer
local fn = vim.fn
local install_path = fn.stdpath('data') .. '/site/pack/packer/start/packer.nvim'

if fn.empty(fn.glob(install_path)) > 0 then
  packer_bootstrap = fn.system({
    'git',
    'clone',
    '--depth',
    '1',
    'https://github.com/wbthomason/packer.nvim',
    install_path
  })
end

-- Autocommand that reloads neovim whenever packer_init.lua file is saved
vim.cmd [[
  augroup packer_user_config
    autocmd!
    autocmd BufWritePost packer_init.lua source <afile> | PackerSync
  augroup end
]]

-- Use protected call so we don't error out on the first use
local status_ok, packer = pcall(require, 'packer')
if not status_ok then
  return
end

return require('packer').startup(function(use)
  -- Add you plugins here:
  use 'wbthomason/packer.nvim' -- packer can manage itself

  -- Colorcshemes
  use 'themercorp/themer.lua'

  -- Telescope
  use {
      'nvim-telescope/telescope.nvim',
      requires = {
          'nvim-lua/plenary.nvim',
          'nvim-telescope/telescope-file-browser.nvim',
          'nvim-telescope/telescope-ui-select.nvim',
          { 'nvim-telescope/telescope-fzf-native.nvim', run = 'make' },
          'nvim-telescope/telescope-github.nvim',
          {
            'nvim-telescope/telescope-frecency.nvim',
            requires = {'tami5/sqlite.lua'},
          },
      }
  }

  -- Treesitter
  use {
      'nvim-treesitter/nvim-treesitter', run = ':TSUpdate',
      requires = {
          'nvim-treesitter/playground',
          'p00f/nvim-ts-rainbow',
          'nvim-treesitter/nvim-treesitter-textobjects',
      },
  }

  -- LSP config
  use {
    'neovim/nvim-lspconfig',
    'williamboman/nvim-lsp-installer',
    'ray-x/lsp_signature.nvim',
  }

  -- Neo-tree
  use {
      'nvim-neo-tree/neo-tree.nvim',
      branch = 'v2.x',
      requires = {
          'nvim-lua/plenary.nvim',
          'kyazdani42/nvim-web-devicons',
          'MunifTanjim/nui.nvim',
      },
      config = function() require('plugins.neo-tree').config() end
  }

  -- Autocomplete
  use {
      'hrsh7th/nvim-cmp',
      requires = {
          'hrsh7th/cmp-nvim-lsp',
          'hrsh7th/cmp-nvim-lua',
          'hrsh7th/cmp-buffer',
          'hrsh7th/cmp-path',
          'saadparwaiz1/cmp_luasnip',
          'L3MON4D3/LuaSnip',
      },
  }
  use 'onsails/lspkind-nvim'

  -- Autopairs
  use 'windwp/nvim-autopairs'

  -- Lsp colors
  use {
    'folke/lsp-colors.nvim',
    config = function ()
      require('lsp-colors').setup()
    end
  }

  -- Statusline
  use {
      'nvim-lualine/lualine.nvim',
      requires = {
          {
              'lewis6991/gitsigns.nvim',
              requires = { 'nvim-lua/plenary.nvim' },
              config = function()
                  require('gitsigns').setup()
              end,
          },
          {
              'kyazdani42/nvim-web-devicons',
              config = function()
                  require('nvim-web-devicons').setup()
              end,
              opt = true,
          },
          {
            'SmiteshP/nvim-gps',
            config = function ()
              require('nvim-gps').setup()
            end
          }
      },
  }

  -- git
  use {
    'TimUntersberger/neogit',
    requires = {
      'nvim-lua/plenary.nvim',
      'sindrets/diffview.nvim',
    },
    config = function ()
      require('plugins.neogit')
    end
  }

  -- -- project
  -- use {
  --   'ahmedkhalf/project.nvim',
  --   config = function ()
  --     require('project_nvim').setup()
  --   end,
  -- }

  -- Terminal
  use {
    'akinsho/nvim-toggleterm.lua',
    config = function ()
      require('plugins.toggleterm').config()
    end
  }

  -- Colorizer
  use {
    'norcalli/nvim-colorizer.lua',
    config = function ()
      require('plugins.colorizer').config()
    end
  }

  -- Comments
  use {
    'numToStr/Comment.nvim',
    config = function() require('Comment').setup() end,
  }

  -- Blankline
  -- use {
  --     'lukas-reineke/indent-blankline.nvim',
  --     config = function ()
  --         require('indent_blankline').setup {
  --             char = '┊',
  --             show_first_indent_level = false,
  --             show_trailing_blankline_indent = false,
  --             show_current_context = true,
  --             show_current_context_start = true,
  --             use_treesitter = true,
  --         }
  --     end
  -- }

  -- Which key
  use 'folke/which-key.nvim'

  use { 'knubie/vim-kitty-navigator' }

  -- Aerial (code jumping)
  use {
    'stevearc/aerial.nvim',
    config = function()
      require('plugins.aerial').setup()
    end,
  }

  -- Language specific
  -- use {
  --   'ray-x/go.nvim',
  --   config = function ()
  --     require('plugins.lsp.go').setup()
  --   end
  -- }

  -- Test
  use {
    'ThePrimeagen/refactoring.nvim',
    config = function ()
      require('refactoring').setup()
    end,
  }

  -- Automatically set up your configuration after cloning packer.nvim
  -- Put this at the end after all plugins
  if packer_bootstrap then
    require('packer').sync()
  end
end)
