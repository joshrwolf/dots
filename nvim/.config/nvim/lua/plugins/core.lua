local Util = require("lazyvim.util")

return {
  -- disable things
  { "rcarriga/nvim-notify", enabled = false },
  { "echasnovski/mini.ai", enabled = false },
  { "echasnovski/mini.surround", enabled = false },
  { "echasnovski/mini.indentscope", enabled = false },
  { "echasnovski/mini.pairs", enabled = false },
  { "nvimdev/dashboard-nvim", enabled = false },
  { "akinsho/bufferline.nvim", enabled = false },
  { "justinsgithub/wezterm-types" },

  -- customize things
  {
    "folke/noice.nvim",
    opts = function(_, opts)
      opts.presets.lsp_doc_border = false
    end,
  },

  {
    "lukas-reineke/indent-blankline.nvim",
    opts = {
      scope = {
        enabled = true,
        show_exact_scope = true,
        show_start = false,
        highlight = { "SpecialKey", "SpecialKey", "SpecialKey" },
      },
    },
  },

  {
    "nvim-lualine/lualine.nvim",
    opts = {
      options = {
        component_separators = "",
        section_separators = "",
      },
      sections = {
        lualine_a = {},
        lualine_b = { { "branch", icon = "" } },
        lualine_c = {
          { "filetype", icon_only = true, separator = "", padding = { left = 1, right = 0 } },
          { "filename", path = 0 },
          {
            "diagnostics",
            symbols = {
              error = require("lazyvim.config").icons.diagnostics.Error,
              warn = require("lazyvim.config").icons.diagnostics.Warn,
              info = require("lazyvim.config").icons.diagnostics.Info,
              hint = require("lazyvim.config").icons.diagnostics.Hint,
            },
          },
          -- disable navic
        },
        lualine_z = {},
      },
    },
  },

  {
    "nvim-treesitter/nvim-treesitter",
    opts = {
      textobjects = {
        select = {
          enable = true,
          lookahead = true,
          include_surrounding_whitespace = false,
          indent = { enable = true },
          keymaps = {
            ["ak"] = { query = "@block.outer", desc = "around block" },
            ["ik"] = { query = "@block.inner", desc = "inside block" },
            ["aa"] = { query = "@parameter.outer", desc = "around argument" },
            ["ia"] = { query = "@parameter.inner", desc = "inside argument" },
            ["af"] = { query = "@function.outer", desc = "around function" },
            ["if"] = { query = "@function.inner", desc = "inside function" },
            ["ac"] = { query = "@class.outer", desc = "around class" },
            ["ic"] = { query = "@class.inner", desc = "inside class" },
            ["ao"] = { query = "@conditional.outer", desc = "around conditional" },
            ["io"] = { query = "@conditional.inner", desc = "inside conditional" },
          },
        },
        move = {
          enable = true,
          set_jumps = false,
          goto_next_start = {
            ["]f"] = "@function.outer",
            ["]a"] = "@parameter.inner",
          },
          goto_previous_start = {
            ["[f"] = "@function.outer",
            ["[a"] = "@parameter.inner",
          },
        },
      },
    },
  },

  {
    "neovim/nvim-lspconfig",
    opts = {
      inlay_hints = { enabled = true },
      format = {
        timeout_ms = 200,
      },
      servers = {
        terraformls = {
          settings = {
            terraformls = {
              prefillRequiredFields = true,
            },
          },
        },
        gopls = {
          settings = {
            gopls = {
              usePlaceholders = false,
              analyses = {
                fieldalignment = false,
                unusedparams = true,
                unusedvariables = true,
              },
              hints = {
                constantValues = false,
                functionTypeParameters = true,
                parameterNames = true,
                rangeVariableTypes = true,
                assignVariableTypes = false,
              },
              codelenses = {
                generate = true,
                tidy = true,
              },
            },
          },
        },
      },
    },
  },

  {
    "mfussenegger/nvim-lint",
    enabled = true,
    -- opts = {
    --   linters_by_ft = {
    --     tf = { ""}
    --   }
    -- },
  },

  {
    "lewis6991/gitsigns.nvim",
    opts = {
      on_attach = function(buf)
        local gs = package.loaded.gitsigns

        local function map(mode, l, r, desc)
          vim.keymap.set(mode, l, r, { buffer = buf, desc = desc })
        end

        vim.keymap.set("n", "[c", function()
          if vim.wo.diff then
            return "[czz"
          end
          vim.schedule(function()
            gs.prev_hunk()
            vim.fn.feedkeys("zz")
          end)
          return "<Ignore>"
        end, { desc = "Prev Hunk", expr = true, buffer = buf })

        vim.keymap.set("n", "]c", function()
          if vim.wo.diff then
            return "]czz"
          end
          vim.schedule(function()
            gs.next_hunk()
            vim.fn.feedkeys("zz")
          end)
          return "<Ignore>"
        end, { desc = "Next Hunk", expr = true, buffer = buf })

        -- stylua: ignore start
        map({ "n", "v" }, "<leader>ghs", ":Gitsigns stage_hunk<CR>", "Stage Hunk")
        map({ "n", "v" }, "<leader>gr", ":Gitsigns reset_hunk<CR>", "Reset Hunk")
        map("n", "<leader>ghS", gs.stage_buffer, "Stage Buffer")
        map("n", "<leader>ghu", gs.undo_stage_hunk, "Undo Stage Hunk")
        map("n", "<leader>gR", gs.reset_buffer, "Reset Buffer")
        map("n", "<leader>gp", gs.preview_hunk, "Preview Hunk")
        map("n", "<leader>gl", function() gs.blame_line({ full = true }) end, "Blame Line")
        map("n", "<leader>ghd", gs.diffthis, "Diff This")
        map("n", "<leader>ghD", function() gs.diffthis("~") end, "Diff This ~")
        map({ "o", "x" }, "ih", ":<C-U>Gitsigns select_hunk<CR>", "GitSigns Select Hunk")
      end,
    },
  },

  {
    "nvim-telescope/telescope.nvim",
    dependencies = {
      {
        "nvim-telescope/telescope-file-browser.nvim",
        keys = {
          { "-", "<cmd>Telescope file_browser path=%:p:h<cr>", desc = "Browser" },
        },
        config = function()
          local actions = require("telescope.actions")
          local fb_actions = require("telescope._extensions.file_browser.actions")
          require("telescope").setup({
            extensions = {
              file_browser = {
                theme = "ivy",
                hidden = true,
                layout_config = { height = 50 },
                hide_parent_dir = true,
                collapse_dirs = false,
                initial_mode = "normal",
                mappings = {
                  ["i"] = {
                    ["<cr>"] = actions.select_default,
                    ["<c-x>"] = actions.select_horizontal,
                    ["<c-v>"] = actions.select_vertical,
                    ["<c-h>"] = fb_actions.goto_parent_dir,
                    ["<c-l>"] = actions.select_default,
                  },
                  ["n"] = {
                    ["<cr>"] = actions.select_default,
                    ["<c-x>"] = actions.select_horizontal,
                    ["<c-v>"] = actions.select_vertical,
                    ["<c-t>"] = actions.select_tab,
                    ["q"] = actions.close,
                    ["<c-c>"] = actions.close,
                    ["h"] = fb_actions.goto_parent_dir,
                    ["l"] = actions.select_default,
                    ["n"] = fb_actions.create_from_prompt,
                    ["."] = fb_actions.toggle_hidden,
                    ["-"] = actions.close,
                  },
                },
              },
            },
          })
          require("telescope").load_extension("file_browser")
        end,
      },
      {
        "nvim-telescope/telescope-fzf-native.nvim",
        build = "make",
        config = function()
          require("telescope").load_extension("fzf")
        end,
      },
    },
    keys = {
      -- find
      { "<leader>ff", "<cmd>Telescope find_files<cr>", desc = "Find files" },
      { "<leader>fo", "<cmd>Telescope oldfiles<cr>", desc = "Recent" },
      { "<leader>fO", Util.telescope("oldfiles", { cwd = vim.loop.cwd() }), desc = "Recent (cwd)" },
      { '<leader>f"', "<cmd>Telescope registers<cr>", desc = "Registers" },
      { "<leader>fa", "<cmd>Telescope autocommands<cr>", desc = "Auto Commands" },
      { "<leader>fb", "<cmd>Telescope current_buffer_fuzzy_find<cr>", desc = "Buffer" },
      { "<leader>fc", "<cmd>Telescope command_history<cr>", desc = "Commtnd History" },
      { "<leader>fC", "<cmd>Telescope commands<cr>", desc = "Commands" },
      { "<leader>fd", "<cmd>Telescope diagnostics bufnr=0<cr>", desc = "Document diagnostics" },
      { "<leader>fD", "<cmd>Telescope diagnostics<cr>", desc = "Workspace diagnostics" },
      { "<leader>fw", "<cmd>Telescope live_grep<cr>", desc = "Grep (root dir)" },
      { "<leader>fW", Util.telescope("live_grep", { cwd = false }), desc = "Grep (cwd)" },
      { "<leader>f;", "<cmd>Telescope resume<cr>", desc = "Resume" },
      { "<leader>uC", Util.telescope("colorscheme", { enable_preview = true }), desc = "Colorscheme with preview" },
      {
        "<leader>fs",
        Util.telescope("lsp_document_symbols", {
          symbols = {
            "Class",
            "Function",
            "Method",
            "Constructor",
            "Interface",
            "Module",
            "Struct",
            "Trait",
            "Field",
            "Property",
          },
        }),
        desc = "Goto Symbol",
      },
      {
        "<leader>fS",
        Util.telescope("lsp_dynamic_workspace_symbols", {
          symbols = {
            "Class",
            "Function",
            "Method",
            "Constructor",
            "Interface",
            "Module",
            "Struct",
            "Trait",
            "Field",
            "Property",
          },
        }),
        desc = "Goto Symbol (Workspace)",
      },
    },
    opts = function()
      local actions = require("telescope.actions")

      return {
        defaults = {
          prompt_prefix = " ",
          selection_caret = " ",
          path_display = { "truncate" },
          sorting_strategy = "ascending",
          layout_config = {
            horizontal = { prompt_position = "top", preview_width = 0.55 },
            vertical = { mirror = false },
            width = 0.87,
            height = 0.80,
            preview_cutoff = 120,
          },
          enable_preview = false,
          file_ignore_patterns = {
            "^node_modules/",
            "third_party/",
            "^.git/",
            "^.intellij/",
            "vendor",
            "packer_compiled",
            "%.DS_Store",
            "%.ttf",
            "%.png",
            "^site-packages/",
            "^.yarn/",
          },
          mappings = {
            i = {
              ["<C-n>"] = actions.cycle_history_next,
              ["<C-p>"] = actions.cycle_history_prev,
              ["<C-j>"] = actions.move_selection_next,
              ["<C-k>"] = actions.move_selection_previous,
            },
            n = {
              ["<cr>"] = actions.select_default,
              q = actions.close,
              ["<c-x>"] = actions.select_horizontal,
              ["<c-v>"] = actions.select_vertical,
              ["<c-t>"] = actions.select_tab,
              ["<c-c>"] = actions.close,
            },
          },
        },
        pickers = {
          find_files = { hidden = true },
          -- find_files = { preview = false },
          buffers = {
            sort_mru = true,
            sort_lastused = true,
            ignore_current_buffer = true,
          },
        },
        -- Define these in their respective extension modules
        -- extensions = {},
      }
    end,
  },

  {
    "L3MON4D3/LuaSnip",
    keys = function()
      -- TODO: Define some keymaps
      return {}
    end,
  },
  {
    "hrsh7th/nvim-cmp",
    opts = function(_, opts)
      local cmp = require("cmp")
      local compare = require("cmp.config.compare")
      opts.completion = {
        completeopt = "menu,menuone,noinsert",
      }
      opts.sorting = {
        comparators = {
          compare.score,
          compare.order,
          compare.recently_used,
          compare.locality,
        },
      }
      opts.sources = cmp.config.sources({
        { name = "nvim_lsp" },
        { name = "path" },
      }, {
        { name = "buffer", keyword_length = 3 },
      })
      opts.confirm_opts = {
        behavior = cmp.ConfirmBehavior.Replace,
      }
      opts.formatting = {
        format = function(_, item)
          local icons = require("lazyvim.config").icons.kinds
          if icons[item.kind] then
            item.kind = icons[item.kind] .. item.kind
          end
          return item
        end,
      }
      opts.mapping = cmp.mapping.preset.insert({
        ["<C-j>"] = cmp.mapping.select_next_item({ behavior = cmp.SelectBehavior.Insert }),
        ["<C-k>"] = cmp.mapping.select_prev_item({ behavior = cmp.SelectBehavior.Insert }),
        ["<C-u>"] = cmp.mapping.scroll_docs(-4),
        ["<C-d>"] = cmp.mapping.scroll_docs(4),
        ["<C-Space>"] = cmp.mapping.complete(),
        ["<C-e>"] = cmp.mapping.abort(),
        -- ["<C-h>"] = vim.lsp.buf.signature_help,
        ["<CR>"] = cmp.mapping.confirm({ select = true }), -- Accept currently selected item. Set `select` to `false` to only confirm explicitly selected items.
      })
      vim.api.nvim_set_hl(0, "CmpGhostText", { link = "Comment", default = true })
      opts.experimental = {
        ghost_text = {
          hl_group = "CmpGhostText",
        },
      }
    end,
  },

  -- add things
  {
    "catppuccin/nvim",
    name = "catppuccin",
    lazy = false,
    priority = 1000,
    config = function()
      require("catppuccin").setup({
        flavour = "mocha",
        integrations = {
          neogit = true,
          barbecue = {
            dim_context = true,
          },
        },

        color_overrides = {
          mocha = {
            -- rosewater = "#E67E80",
            -- flamingo = "#E67E80",
            pink = "#D699B6", -- purple
            mauve = "#E69875", -- orange
            lavender = "#D3C6AA", -- fg
            red = "#E67E80", -- red
            maroon = "#D8D3BA", -- make this a darker fg0
            -- peach = "#93B259",
            yellow = "#DBBC7F", -- yellow
            green = "#A7C080", -- green:
            teal = "#7fbbb3",
            sky = "#D3C6AA", -- fg0: :=
            -- sapphire = "#A7C080",
            blue = "#83C092", -- blue
            text = "#D3C6AA", -- fg0: correct
            subtext0 = "#bdae93",
            subtext1 = "#d5c4a1",
            overlay0 = "#7A8478", -- grey0: maybe?
            overlay1 = "#859289", -- grey1: maybe?
            overlay2 = "#9DA9A0", --  grey2: maybe?
            surface0 = "#3D484D", -- bg1: maybe?
            surface1 = "#4F585E", -- bg2: maybe?
            surface2 = "#475258", -- bg3: maybe?
            base = "#333C43", -- low bg0: correct
            mantle = "#3a464c",
            crust = "#4d5960",
          },
        },
        highlight_overrides = {
          mocha = function(mocha)
            return {
              ["@namespace"] = { style = { "bold" } },
              LineNr = { fg = "#859289" },
              CursorLineNr = { style = { "bold" } },
              FloatBorder = { fg = "#859289" }, -- for things like the telescope border
              ["@field.yaml"] = { fg = "#a7c080" },
              ["@string.yaml"] = { fg = "#d3c6aa" },
              ["@definition.cue"] = { link = "@function" },
            }
          end,
        },
      })

      vim.cmd("colorscheme catppuccin")
    end,
  },

  -- {
  --   "projekt0n/github-nvim-theme",
  --   lazy = false, -- make sure we load this during startup if it is your main colorscheme
  --   priority = 1000, -- make sure to load this before all the other start plugins
  --   config = function()
  --     require("github-theme").setup({
  --       -- ...
  --     })
  --
  --     vim.cmd("colorscheme github_dark")
  --   end,
  -- },

  -- Git things
  {
    "tpope/vim-fugitive",
    cmd = { "G", "Git", "Gbrowse" },
    keys = {
      --   { "<leader>gg", ":tab G<cr>", desc = "Fugitive" },
      --   { "<leader>ga", ":Gwrite<cr>", desc = "Write file" },
      { "<leader>gx", ":GBrowse @upstream<cr>", desc = "Open in GH @upstream", mode = { "n", "v" } },
      { "<leader>gX", ":GBrowse<cr>", desc = "Open in GH @origin", mode = { "n", "v" } },
    },
    dependencies = {
      { "tpope/vim-rhubarb" },
    },
    config = function()
      vim.api.nvim_create_autocmd("FileType", {
        pattern = "fugitive",
        callback = function(ctx)
          vim.keymap.set("n", "<Tab>", "=", { remap = true, buffer = ctx.buf })
          vim.keymap.set("n", "q", ":q<cr>", { buffer = ctx.buf })
          vim.keymap.set("n", "dt", ":Gtabedit <Plug><cfile><Bar>Gdiffsplit<CR>", { buffer = ctx.buf })
          vim.keymap.set("n", "S", "<cmd>silent !git add -A<cr>", { buffer = ctx.buf, desc = "Add All" })
        end,
      })
    end,
  },
  {
    "rbong/vim-flog",
    cmd = { "Flog", "Flogsplit" },
    keys = {
      { "<leader>gL", "<cmd>Flog<cr>", "n", desc = "Git log" },
    },
    dependencies = {
      "tpope/vim-fugitive",
    },
  },
  {
    "sindrets/diffview.nvim",
    cmd = { "DiffviewOpen", "DiffviewClose", "DiffviewToggleFiles", "DiffviewFocusFiles" },
    keys = {
      { "<leader>gd", "<cmd>DiffviewOpen<cr>", desc = "Diffview" },
    },
    config = function()
      require("diffview").setup({})
    end,
  },
  {
    "NeogitOrg/neogit",
    dependencies = {
      "nvim-lua/plenary.nvim", -- required
      "nvim-telescope/telescope.nvim", -- optional
      "sindrets/diffview.nvim", -- optional
    },
    keys = {
      { "<leader>gg", "<cmd>Neogit<cr>", desc = "Git" },
    },
    opts = function(_, opts)
      return {
        telescope_sorter = function()
          return require("telescope").extensions.fzf.native_fzf_sorter()
        end,
        integrations = {
          telescope = true,
          diffview = true,
        },
      }
    end,
  },
  {
    "Wansmer/treesj",
    keys = {},
    cmd = { "TSJToggle", "TSJSplit", "TSJJoin" },
    opts = { use_default_keymaps = false },
  },

  -- Stolen from AstroVim
  {
    "NvChad/nvim-colorizer.lua",
    event = "VeryLazy",
    cmd = { "ColorizerToggle", "ColorizerAttachToBuffer", "ColorizerDetachFromBuffer", "ColorizerReloadAllBuffers" },
    opts = { user_default_options = { names = false } },
  },

  {
    "stevearc/oil.nvim",
    dependencies = { "nvim-tree/nvim-web-devicons" },
    cmd = "Oil",
    keys = {
      {
        "<leader>-",
        function()
          require("oil").open()
        end,
        desc = "Open folder in Oil",
      },
    },
    opts = {
      view_options = {
        show_hidden = true,
        is_always_hidden = function(name, bufnr)
          return (name == "..")
        end,
      },
      float = {
        win_options = { winblend = 0 },
      },
      keymaps = {
        ["g?"] = "actions.show_help",
        ["<CR>"] = "actions.select",
        ["<C-v>"] = "actions.select_vsplit",
        ["<C-s>"] = "actions.select_split",
        ["<C-t>"] = "actions.select_tab",
        ["<C-f>"] = "actions.preview",
        ["<C-c>"] = "actions.close",
        ["-"] = "actions.parent",
        ["_"] = "actions.open_cwd",
        ["`"] = "actions.cd",
        ["~"] = "actions.tcd",
        ["g."] = "actions.toggle_hidden",
      },
      use_default_keymaps = false,
    },
  },

  -- Other
  {
    "rgroli/other.nvim",
    keys = {
      { "ga", "<cmd>:Other<cr>", desc = "Other" },
    },
    config = function()
      require("other-nvim").setup({
        mappings = {
          "golang",
        },
      })
    end,
  },

  -- Ripped from AstroVim
  {
    "windwp/nvim-autopairs",
    event = "InsertEnter",
    opts = {
      check_ts = true,
      ts_config = { java = false },
      fast_wrap = {
        map = "<M-e>",
        chars = { "{", "[", "(", '"', "'" },
        pattern = string.gsub([[ [%'%"%)%>%]%)%}%,] ]], "%s+", ""),
        offset = 0,
        end_key = "$",
        keys = "qwertyuiopzxcvbnmasdfghjkl",
        check_comma = true,
        highlight = "PmenuSel",
        highlight_grey = "LineNr",
      },
    },
    config = function(_, opts)
      local npairs = require("nvim-autopairs")
      npairs.setup(opts)

      local cmp_status_ok, cmp = pcall(require, "cmp")
      if cmp_status_ok then
        cmp.event:on("confirm_done", require("nvim-autopairs.completion.cmp").on_confirm_done({ tex = false }))
      end
    end,
  },

  {
    "utilyre/barbecue.nvim",
    name = "barbecue",
    version = "*",
    dependencies = {
      "SmiteshP/nvim-navic",
      "nvim-tree/nvim-web-devicons", -- optional dependency
    },
    opts = {
      show_modified = true,
      symbols = {
        separator = "",
      },
    },
  },

  {
    "kylechui/nvim-surround",
    version = "*", -- Use for stability; omit to use `main` branch for the latest features
    event = "VeryLazy",
    config = function()
      require("nvim-surround").setup({
        -- Configuration here, or leave empty to use defaults
      })
    end,
  },

  -- Obsidian
  {
    "epwalsh/obsidian.nvim",
    dependencies = {
      "nvim-lua/plenary.nvim",
      "hrsh7th/nvim-cmp",
      "nvim-telescope/telescope.nvim",
    },
    keys = {
      { "<leader>fnw", "<cmd>ObsidianSearch<cr>", "Search Obsidian" },
      { "<leader>fnn", "<cmd>ObsidianNew<cr>", "New Notes" },
      { "<leader>fnd", "<cmd>ObsidianToday<cr>", "Daily Note" },
      { "<leader>fns", "<cmd>ObsidianYesterday<cr>", "Yesterday Note" },
    },
    opts = {
      dir = "~/.brain",
      use_advanced_uri = true,
      daily_notes = { folder = "dailies" },
      note_frontmatter_func = function(note)
        -- This is equivalent to the default frontmatter function.
        local out = { id = note.id, aliases = note.aliases, tags = note.tags }
        -- `note.metadata` contains any manually added fields in the frontmatter.
        -- So here we just make sure those fields are kept in the frontmatter.
        if note.metadata ~= nil and require("obsidian").util.table_length(note.metadata) > 0 then
          for k, v in pairs(note.metadata) do
            out[k] = v
          end
        end
        return out
      end,
    },
  },

  {
    "github/copilot.vim",
    config = function()
      vim.keymap.set("i", "<c-r>", 'copilot#Accept("<CR>")', {
        expr = true,
        replace_keycodes = false,
      })
      vim.g.copilot_no_tab_map = true
    end,
  },
  {
    "jackMort/ChatGPT.nvim",
    dependencies = {
      "MunifTanjim/nui.nvim",
      "nvim-lua/plenary.nvim",
      "nvim-telescope/telescope.nvim",
    },
    event = "VeryLazy",
    config = function()
      require("chatgpt").setup({
        openai_params = {
          model = "gpt-4-1106-preview",
        },
      })
    end,
  },
}
