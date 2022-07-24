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
  -- use {
  --   "rmehri01/onenord.nvim",
  --   config = function ()
  --     require("onenord").setup({
  --       theme = "light",
  --       fade_nc = false,
  --       styles = {
  --         comments = "ITALIC",
  --       }
  --     }) 
  --   end
  -- }
  -- use {
  --   "EdenEast/nightfox.nvim",
  --   config = function()
  --     require("config.colors.nightfox").setup()
  --   end
  -- }
  use {
    "projekt0n/github-nvim-theme",
    config = function ()
      require("github-theme").setup({
        theme_style = "dark",
      }) 
    end
  }
  -- use {
  --   "catppuccin/nvim",
  --   as = "catppuccin",
  --   config = function ()
  --     require("config.colors.catpuccin") .setup()
  --   end
  -- }
  -- use {
  --   "ellisonleao/gruvbox.nvim",
  --   config = function ()
  --     require("gruvbox").setup({
  --       italic = false,
  --       contrast = "soft",
  --     })
  --     vim.o.background = "light"
  --     vim.cmd[[colorscheme gruvbox]]
  --   end
  -- }
  -- use {
  --   "sainnhe/gruvbox-material",
  --   config = function ()
  --     vim.o.background = "light"
  --     vim.g.gruvbox_material_background = "medium"
  --     vim.g.gruvbox_material_better_performance = 1
  --     vim.cmd[[colorscheme gruvbox-material]] 
  --   end
  -- }
  -- use({
  --   "themercorp/themer.lua",
  --   config = function()
  --   require("themer").setup({
  --     colorscheme = "everforest",
  --     -- styles = {
  --     --   ["function"] = { style = 'italic' },
  --     --   functionbuiltin = { style = 'italic' },
  --     --   variable = { style = 'italic' },
  --     --   variableBuiltIn = { style = 'italic' },
  --     --   parameter  = { style = 'italic' },
  --     -- },
  --     dim_inactive = true,
  --   })
  --   end
  -- })
  -- use {
  --   "sainnhe/everforest",
  --   config = function ()
  --     vim.o.background = "light"
  --     vim.g.everforest_background = "soft"
  --     vim.g.everforest_better_performance = 1
  --     vim.cmd[[colorscheme everforest]]
  --   end
  -- }
  -- use {
  --   "mcchrish/zenbones.nvim",
  --   requires = "rktjmp/lush.nvim",
  --   config = function ()
  --     vim.o.background = "light" 
  --     vim.cmd[[colorscheme forestbones]]
  --   end
  -- }
  -- use({
  --   'projekt0n/github-nvim-theme',
  --   config = function()
  --     require('github-theme').setup({
  --       theme_style = "light_default"
  --     })
  --   end
  -- })

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
  -- use {
  --   "kyazdani42/nvim-tree.lua",
  --   requires = {
  --     "kyazdani42/nvim-web-devicons",
  --   },
  --   config = function ()
  --     require("config.neotree").setup() 
  --   end
  -- }

  -- Statusline
  -- use {
  --   "feline-nvim/feline.nvim",
  --   after = "nvim-web-devicons",
  --   config = function ()
  --     require("config.feline").setup()
  --   end
  -- }
  use {
    "nvim-lualine/lualine.nvim",
    requires = {
      "kyazdani42/nvim-web-devicons",
    },
    config = function ()
      require("lualine").setup()
    end
  }

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

  -- Builtin LSP
  use {
    "neovim/nvim-lspconfig",
    -- event = "VimEnter",
    requires = {
      "folke/lua-dev.nvim",
      "b0o/schemastore.nvim",
      "williamboman/nvim-lsp-installer",
      "tamago324/nlsp-settings.nvim",
      {
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
    },
    config = function()
      require("config.lsp").setup()
    end
  }

  -- Formatting and linting
  -- use {
  --   "jose-elias-alvarez/null-ls.nvim",
  --   event = { "BufRead", "BufNewFile" },
  --   config = function()
  --     require("config.lsp.null-ls").setup()
  --   end
  -- }

  -- Autocomplete
  use {
    "hrsh7th/nvim-cmp",
    event = "InsertEnter",
    -- opt = true,
    keys = {
      { "n", ":" },
      { "n", "/" },
    },
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
        requires = {
          "rafamadriz/friendly-snippets",
          "honza/vim-snippets",
        },
      },
    },
  }

  use {
    "windwp/nvim-autopairs",
    opt = true,
    event = "InsertEnter",
    module = { "nvim-autopairs.completion.cmp", "nvim-autopairs" },
    config = function()
      require("nvim-autopairs").setup {
        -- map_cr = false,
        enable_check_bracket_line = false,
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

  -- Github integrations
  use {
    "pwntester/octo.nvim",
    cmd = "Octo",
    wants = { "telescope.nvim", "plenary.nvim", "nvim-web-devicons" },
    requires = {
      "nvim-lua/plenary.nvim",
      "nvim-telescope/telescope.nvim",
      "kyazdani42/nvim-web-devicons"
    },
    config = function ()
      require("octo").setup()
    end
  }

  -- symbols-outline
  use {
    "simrat39/symbols-outline.nvim",
    cmd = { "SymbolsOutline" },
    config = function ()
      vim.g.symbols_outline = {
        auto_preview = false,
      }
    end
  }

  -- Parenthesis highlighting
  -- use {
  --   "p00f/nvim-ts-rainbow",
  --   after = "nvim-treesitter",
  -- }

  -- Syntax highlighting
  use {
    "nvim-treesitter/nvim-treesitter",
    run = ":TSUpdate",
    event = { "BufRead", "BufNewFile" },
    config = function()
      require("config.treesitter").setup()
    end,
    requires = {
      { "nvim-treesitter/nvim-treesitter-textobjects", event = "BufRead" },
      { "nvim-treesitter/playground" },
    },
  }

  -- Indent blankline
  use {
    "lukas-reineke/indent-blankline.nvim",
    event = "BufReadPre",
    config = function ()
      require("indent_blankline").setup {
        filetype_excludes = {
          "lspinfo",
          "packer",
          "checkhealth",
          "help",
          "dashboard",
          "telescope",
          "neo-tree",
        },
        show_current_context = true,
        show_current_context_start = false,
        use_treesitter = true,
      }
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
      "nvim-telescope/telescope-symbols.nvim",
      "nvim-telescope/telescope-ui-select.nvim",
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
    -- cmd = "ToggleTerm",
    config = function ()
      require("config.toggleterm").setup()
    end,
  }

  -- Tmux navigator
  use 'christoomey/vim-tmux-navigator'

  -- go
  -- use {
  --   "fatih/vim-go",
  --   -- ft = { "go" },
  --   config = function ()
  --     -- vim.g.go_code_completion_enabled = 0
  --     vim.g.go_doc_popup_window = 1
  --     vim.g.go_auto_sameids = 0
  --
  --     vim.g.go_highlight_types = 1
  --     vim.g.go_highlight_fields = 1
  --     vim.g.go_highlight_functions = 1
  --     vim.g.go_highlight_function_calls = 1
  --     -- let g:go_list_type="quickfix"
  --     -- let g:go_fmt_command="goimports"
  --     -- let g:go_highlight_types=1
  --     -- let g:go_highlight_fields=1
  --     -- let g:go_highlight_functions=1
  --     -- let g:go_highlight_function_calls=1
  --     -- let g:go_fmt_fail_silently=1
  --   end
  -- }

  -- Leap
  use {
    "ggandor/leap.nvim",
    event = "VimEnter",
    config = function()
      local leap = require("leap")
      leap.setup {
        case_insensitive = false,
      }
      leap.set_default_keymaps()
    end
  }

  use {
    "ThePrimeagen/harpoon",
    config = function()
      require("harpoon").setup{}
    end,
  }

  -- Trials
  use {
    "kylechui/nvim-surround",
    config = function ()
      require("nvim-surround").setup({
      }) 
    end
  }

  use {
    "TimUntersberger/neogit",
    cmd = "Neogit",
    requires = {
      "nvim-lua/plenary.nvim",
      "sindrets/diffview.nvim",
    },
    config = function ()
      require("neogit").setup({
        integrations = {
          diffview = true,
        }
      }) 
    end
  }

  use "tpope/vim-fugitive"

  use {
    "andymass/vim-matchup",
    keys = {
      { "n", "%" },
      { "v", "%" },
    }
  }

  use {
    "obaland/vfiler.vim",
    cmd = "VFiler",
    requires = {
      "obaland/vfiler-column-devicons",
    },
    config = function ()
      require("vfiler/config").setup {
        options = {
          auto_cd = true,
          auto_resize = true,
          columns = "indent,icon,name",
        },
      } 
    end
  }

  use {
    "AndrewRadev/splitjoin.vim",
    keys = {
      { "n", "gJ" },
      { "n", "gS" },
    },
  }

  -- Fix the CursorHold performance bug
  use "antoinemadec/FixCursorHold.nvim"
  
  -- use {
  --   "akinsho/nvim-bufferline.lua",
  --   event = "BufRead",
  --   config = function ()
  --     require("bufferline").setup({
  --       options = {
  --         mode = "tabs",
  --         offsets = {
  --           { filetype = "neo-tree" },
  --         }
  --       }
  --     }) 
  --   end
  -- }
  use {
    "nanozuki/tabby.nvim",
    config = function ()
      require("config.tabby").setup()
    end
  }

  use {
    'luukvbaal/nnn.nvim',
    config = function ()
      require("nnn").setup() 
    end
  }

  -- Maybe
  -- https://github.com/NTBBloodbath/rest.nvim

	-- Automatically set up config after cloning packer.nvim
	if PACKER_BOOTSTRAP then
		require("packer").sync()
	end
end)
