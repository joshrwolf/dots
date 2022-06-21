local function packer_init()
	local packer_avail, packer = pcall(require, "packer")
	if not packer_avail then
		local packer_path = stdpath "data" .. "/site/pack/packer/start/packer.nvim"
		vim.fn.delete(packer_path, "rf")
		vim.fn.system {
			"git",
			"clone",
			"--depth",
			"1",
			"https://github.com/wbthomason/packer.nvim",
			packer_path,
		}
		print("Installing packer...")
		vim.api.nvim_command("packadd packer.nvim")

		packer_avail, avail = pcall(require, "packer")
		if not packer_avail then
			vim.api.nvim_err_writeln("Failed to load packer at:" .. packer_path .. "\n\n" .. packer)
		end
	end
	return packer
end

vim.cmd [[
	augroup packer_user_config
		autocmd!
		autocmd BufWritePost plugins.lua source <afile> | PackerSync
	augroup end
]]

packer = packer_init()

local conf = {
	profile = {
		enable = true,
		threshold = 0,
	},
	display = {
		open_fn = function()
			return require("packer.util").float { border = "rounded" }
		end
	},
}

packer.init(conf)


return packer.startup(function(use)
	-- Plugin Manager
	use { "wbthomason/packer.nvim" }

	-- Optimiser
	use { "lewis6991/impatient.nvim" }

  -- Lua functions
  use {
    "nvim-lua/plenary.nvim",
    module = "plenary",
  }

  -- Notification Enhancer

  -- Neovim UI Enhancer
  use {
    "MunifTanjim/nui.nvim",
    module = "nui",
  }

  -- Icons
  use {
    "kyazdani42/nvim-web-devicons",
    event = "VimEnter",
    config = function()
      require("config.icons").setup()
    end
  }

  -- Bufferline

  -- Commenting
  use {
    "numToStr/Comment.nvim",
    module = {
      "Comment",
      "Comment.api",
    },
    keys = { "gc", "gb", "g<", "g>" },
    config = function()
      require("Comment").setup {}
    end
  }

  -- Colorscheme
  use {
    "EdenEast/nightfox.nvim",
    config = function()
      require("config.colors.nightfox").setup()
    end
  }
  -- use {
  --   "catppuccin/nvim",
  --   as = "catppuccin",
  --   config = function()
  --     require("config.colors.catpuccin").setup()
  --   end
  -- }
  -- use {
  --   "andersevenrud/nordic.nvim",
  --   config = function()
  --     require("nordic").colorscheme({
  --       italic = false,
  --     })
  --   end
  -- }

  -- File explorer
  use {
    "nvim-neo-tree/neo-tree.nvim",
    branch = "v2.x",
    module = "neo-tree",
    cmd = "Neotree",
    requires = {},
    config = function()
      require("config.neotree").setup()
    end
  }

  -- Statusline
  use {
    "feline-nvim/feline.nvim",
    after = "nvim-web-devicons",
    config = function ()
      require("config.feline").setup()
    end
  }

  -- Smooth scrolling
  use {
    "declancm/cinnamon.nvim",
    event = { "BufRead", "BufNewFile" },
    config = function()
      require("cinnamon").setup({
        default_delay = 3,
      })
    end
  }

  -- Smooth escaping

  -- Colorizer
  use {
    "norcalli/nvim-colorizer.lua",
    event = { "BufRead", "BufNewFile" },
    config = function()
      require("colorizer").setup()
    end,
  }

  use {
    "folke/todo-comments.nvim",
    -- cmd = { "TodoQuickfix", "TodoTrouble", "TodoTelescope" },
    config = function()
      require("todo-comments").setup {}
    end
  }

  -- Autocomplete
  use {
    "hrsh7th/nvim-cmp",
    event = "InsertEnter",
    opt = true,
    config = function()
      require("config.cmp").setup()
    end,
    wants = { "LuaSnip" },
    requires = {
      "hrsh7th/cmp-buffer",
      "hrsh7th/cmp-path",
      "hrsh7th/cmp-nvim-lua",
      "ray-x/cmp-treesitter",
      "hrsh7th/cmp-cmdline",
      "saadparwaiz1/cmp_luasnip",
      "hrsh7th/cmp-nvim-lsp",
      "hrsh7th/cmp-nvim-lsp-signature-help",
      {
        "L3MON4D3/LuaSnip",
        wants = { "friendly-snippets", "vim-snippets" },
        config = function()
          -- require("config.snip").setup()
        end,
      },
      "rafamadriz/friendly-snippets",
      "honza/vim-snippets",
    },
  }

  -- Lsp status
  use {
    "j-hui/fidget.nvim",
    config = function ()
      require("fidget").setup{
        text = {
          spinner = "dots",
          done = "",
        }
      } 
    end
  }

  use {
    "windwp/nvim-autopairs",
    opt = true,
    event = "InsertEnter",
    module = { "nvim-autopairs.completion.cmp", "nvim-autopairs" },
    config = function()
      local npairs = require "nvim-autopairs"
      npairs.setup {
        check_ts = true,
      }
    end
  }

  -- Git integrations
  use {
    "lewis6991/gitsigns.nvim",
    event = "BufEnter",
    config = function ()
      require("config.gitsigns").setup() 
    end
  }

  -- Parenthesis highlighting
  use {
    "p00f/nvim-ts-rainbow",
    after = "nvim-treesitter",
  }

  -- Syntax highlighting
  use {
    "nvim-treesitter/nvim-treesitter",
    run = ":TSUpdate",
    event = { "BufRead", "BufNewFile" },
    cmd = {
      "TSInstall",
      "TSInstallInfo",
      "TSInstallSync",
      "TSUninstall",
      "TSUpdate",
      "TSUpdateSync",
      "TSDisableAll",
      "TSEnableAll",
    },
    config = function()
      require("config.treesitter").setup()
    end,
    requires = {
      "nvim-treesitter/playground",
    },
  }

  -- Builtin LSP
  use {
    "neovim/nvim-lspconfig",
    event = "VimEnter",
  }

  -- Formatting and linting
  use {
    "jose-elias-alvarez/null-ls.nvim",
    event = { "BufRead", "BufNewFile" },
    config = function()
      require("config.lsp.null-ls").setup()
    end
  }

  -- LSP manager
  use {
    "williamboman/nvim-lsp-installer",
    after = "nvim-lspconfig",
    config = function()
      require("config.lsp").setup()
    end
  }

  -- Fuzzy Finder
  use {
    "nvim-telescope/telescope.nvim",
    cmd = "Telescope",
    module = "telescope",
    config = function()
      require("config.telescope").setup()
    end,
    wants = {
      "plenary.nvim",
    },
    requires = {
      { "nvim-telescope/telescope-fzf-native.nvim", run = "make" },
      "nvim-telescope/telescope-file-browser.nvim",
      { "nvim-telescope/telescope-frecency.nvim", requires = "tami5/sqlite.lua" },
    },
  }

  -- Keymaps
  use {
    "folke/which-key.nvim",
    event = "VimEnter",
    module = "which-key",
    config = function()
      require("config.whichkey").setup()
    end
  }

  -- Trouble
  use {
    "folke/trouble.nvim",
    wants = "nvim-web-devicons",
    config = function()
      require("trouble").setup {
        use_diagnostic_signs = true,
      }
    end
  }

  -- Terminal
  use {
    "akinsho/toggleterm.nvim",
    cmd = { "ToggleTerm", "TermExec" },
    module = { "toggleterm", "toggleterm.terminal" },
    config = function ()
      require("config.toggleterm").setup() 
    end,
  }

  -- Tmux navigator
  use 'christoomey/vim-tmux-navigator'

  -- go
  use {
    "ray-x/go.nvim",
    event = "VimEnter",
    ft = { "go" },
    config = function()
      require("config.go").setup()
    end
  }

  -- Trials
  use {
    "ggandor/leap.nvim",
    event = "VimEnter",
    config = function()
      require("leap").set_default_keymaps() 
    end
  }

	-- Automatically set up config after cloning packer.nvim
	if PACKER_BOOTSTRAP then
		require("packer").sync()
	end
end)
