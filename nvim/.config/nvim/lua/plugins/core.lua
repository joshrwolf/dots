return {
  {
    "LazyVim/LazyVim",
    opts = {
      colorscheme = "github_light",
    },
  },

  -- Theme
  {
    "projekt0n/github-nvim-theme",
    lazy = false,
    priority = 1000,
    config = function()
      require("github-theme").setup({
        options = {
          dim_inactive = true,
          styles = {
            -- this is a comment
            comments = "",
            keywords = "",
            functions = "",
          },
          modules = {},
        },
        palettes = {
          github_light = {
            bg0 = "#F6F8FA",
            bg1 = "#000000",
            bg2 = "#F6F8FA",
          },
        },
      })

      vim.cmd("colorscheme github_light")
    end,
  },

  -- Catppuccin with GitHub Light colors mapped onto Latte
  {
    "catppuccin/nvim",
    name = "catppuccin",
    lazy = false,
    priority = 1000,
    opts = {
      flavour = "latte",
      no_italic = true,
      no_bold = true,
      no_underline = true,
      color_overrides = {
        latte = {
          base = "#ffffff",
          mantle = "#f6f8fa",
          crust = "#eaeef2",
          text = "#1F2328",
          subtext1 = "#32383f",
          subtext0 = "#424a53",
          overlay2 = "#57606a",
          overlay1 = "#6e7781",
          overlay0 = "#8c959f",
          surface2 = "#afb8c1",
          surface1 = "#d0d7de",
          surface0 = "#eaeef2",
          red = "#a40e26",
          maroon = "#82071e",
          green = "#116329",
          blue = "#0550ae",
          yellow = "#7d4e00",
          mauve = "#6639ba",
          peach = "#953800",
          pink = "#99286e",
          teal = "#1b7c83",
          sky = "#0969da",
          sapphire = "#033d8b",
          lavender = "#512a97",
          flamingo = "#a40e26",
          rosewater = "#9e2f1c",
        },
      },
      custom_highlights = function(colors)
        return {
          -- Keywords (return, end, function, if, then, else, for, etc.) → red
          ["@keyword"] = { fg = colors.red },
          ["@keyword.return"] = { fg = colors.red },
          ["@keyword.function"] = { fg = colors.red },
          ["@keyword.operator"] = { fg = colors.red },
          ["@keyword.conditional"] = { fg = colors.red },
          ["@keyword.repeat"] = { fg = colors.red },
          ["@keyword.modifier"] = { fg = colors.red },
          ["Keyword"] = { fg = colors.red },
          ["Conditional"] = { fg = colors.red },
          ["Repeat"] = { fg = colors.red },
          ["Statement"] = { fg = colors.red },

          -- Strings → dark blue (GitHub uses blue[9] #0a3069)
          ["@string"] = { fg = "#0a3069" },
          ["String"] = { fg = "#0a3069" },

          -- Booleans, constants, nil → blue
          ["@boolean"] = { fg = colors.blue },
          ["@constant.builtin"] = { fg = colors.blue },
          ["Boolean"] = { fg = colors.blue },
          ["Constant"] = { fg = colors.blue },

          -- Numbers → blue
          ["@number"] = { fg = colors.blue },
          ["Number"] = { fg = colors.blue },

          -- Functions → purple
          ["@function"] = { fg = colors.mauve },
          ["@function.call"] = { fg = colors.mauve },
          ["@function.builtin"] = { fg = colors.mauve },
          ["@method"] = { fg = colors.mauve },
          ["@method.call"] = { fg = colors.mauve },
          ["Function"] = { fg = colors.mauve },

          -- Operators/punctuation → text color
          ["@operator"] = { fg = colors.text },
          ["@punctuation.bracket"] = { fg = colors.text },
          ["@punctuation.delimiter"] = { fg = colors.text },

          -- Comments → muted gray
          ["@comment"] = { fg = colors.overlay1 },
          ["Comment"] = { fg = colors.overlay1 },

          -- Properties/fields/table keys → text
          ["@property"] = { fg = colors.text },
          ["@field"] = { fg = colors.text },
          ["@lsp.type.property"] = { fg = colors.text },

          -- Variables → text
          ["@variable"] = { fg = colors.text },
          ["@variable.builtin"] = { fg = colors.blue },

          -- Types → orange
          ["@type"] = { fg = colors.peach },
          ["@type.builtin"] = { fg = colors.peach },
          ["Type"] = { fg = colors.peach },

          -- LSP inlay hints → darker
          ["LspInlayHint"] = { fg = "#57606a", bg = "#eaeef2" },
        }
      end,
    },
  },

  {
    "saghen/blink.cmp",
    opts = {
      completion = {
        documentation = {
          auto_show_delay_ms = 0,
        },
      },
      sources = {
        default = { "lsp", "path", "buffer" },
      },
      keymap = {
        ["<C-k>"] = { "select_prev" },
        ["<C-j>"] = { "select_next" },
      },
    },
  },

  -- lsp
  {
    "neovim/nvim-lspconfig",
    opts = function(_, opts)
      -- local keys = require("lazyvim.plugins.lsp.keymaps").get()
      -- keys[#keys + 1] = { "<C-k>", false }

      opts.servers = {
        gopls = {
          settings = {
            gopls = {
              usePlaceholders = false,
              analyses = {
                fieldalignment = false,
              },
              hints = {
                assignVariableTypes = false,
                rangeVariableTypes = false,
              },
              directoryFilters = {
                "-.git",
                "-.vscode",
                "-.idea",
                "-.vscode-test",
                "-node_modules",
                "-.terraform",
                "-.claude",
              },
            },
          },
        },
        yamlls = {
          settings = {
            format = {
              enable = false,
            },
          },
        },
        cue = {},
        terraformls = {},
        lua_ls = {
          settings = {
            Lua = {
              diagnostics = {
                globals = { "vim" },
              },
            },
          },
        },
      }
    end,
  },

  {
    "stevearc/conform.nvim",
    opts = {
      default_format_opts = { timeout_ms = 400 },
      formatters_by_ft = {
        cue = { "cue_fmt" },
        d2 = { "d2" },
      },
    },
  },

  -- treesitter
  {
    "nvim-treesitter/nvim-treesitter",
    opts = {
      indent = { enable = false },
      ensure_installed = { "cue", "txtar" },
    },
  },

  -- ui: noice
  {
    "folke/noice.nvim",
    keys = {
      { "<leader>n", "<cmd>NoiceAll<cr>", desc = "Noice" },
    },
    opts = {
      notify = { enabled = false },
      presets = {
        lsp_doc_border = true,
      },
    },
  },

  -- oil
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
    config = function()
      require("oil").setup({
        view_options = {
          show_hidden = true,
        },
      })
    end,
  },

  -- colorize hex codes
  {
    "brenoprata10/nvim-highlight-colors",
    event = "VeryLazy",
    config = true,
  },

  -- statusbar
  {
    "nvim-lualine/lualine.nvim",
    event = "VeryLazy",
    -- opts = {
    --   options = {
    --     component_separators = "",
    --     section_separators = "",
    --   },
    --   sections = {
    --     lualine_a = { "mode", draw_empty = false },
    --     lualine_b = { { "branch", icon = "" } },
    --     lualine_c = {
    --       { "filetype", icon_only = true, separator = "", padding = { left = 1, right = 0 } },
    --       {
    --         "filename",
    --         path = 1,
    --       },
    --     },
    --     lualine_d = {
    --       "filename",
    --     },
    --     lualine_z = {},
    --   },
    -- },
    opts = function()
      -- PERF: we don't need this lualine require madness 🤷
      local lualine_require = require("lualine_require")
      lualine_require.require = require

      local icons = LazyVim.config.icons

      -- Spinner component definition
      local spinner = {
        processing = false,
        spinner_index = 1,
        spinner_symbols = {
          "⠋",
          "⠙",
          "⠹",
          "⠸",
          "⠼",
          "⠴",
          "⠦",
          "⠧",
          "⠇",
          "⠏",
        },
      }

      -- Create autocommands for spinner
      local group = vim.api.nvim_create_augroup("CodeCompanionHooks", {})
      vim.api.nvim_create_autocmd({ "User" }, {
        pattern = "CodeCompanionRequest*",
        group = group,
        callback = function(request)
          if request.match == "CodeCompanionRequestStarted" then
            spinner.processing = true
          elseif request.match == "CodeCompanionRequestFinished" then
            spinner.processing = false
          end
        end,
      })

      vim.o.laststatus = vim.g.lualine_laststatus
      return {
        options = {
          theme = "auto",
          globalstatus = vim.o.laststatus == 3,
          disabled_filetypes = { statusline = { "dashboard", "alpha", "starter" } },
          component_separators = "",
          section_separators = "",
        },
        sections = {
          lualine_a = { "mode", { draw_empty = true } },
          lualine_b = { { "branch", icon = "" } },
          lualine_c = {
            { "filetype", icon_only = true, separator = "", padding = { left = 1, right = 0 } },
            { "filename", path = 1 },
          },
          lualine_x = {
            {
              function()
                if spinner.processing then
                  spinner.spinner_index = (spinner.spinner_index % #spinner.spinner_symbols) + 1
                  return spinner.spinner_symbols[spinner.spinner_index]
                end
                return ""
              end,
            },
          },
        },
        extensions = {
          "lazy",
          "fzf",
          "mason",
          "oil",
          "aerial",
          "overseer",
        },
      }
    end,
  },

  -- winbar
  { "Bekaboo/dropbar.nvim" },

  -- git
  {
    "NeogitOrg/neogit",
    cmd = { "Neogit" },
    dependencies = {
      "nvim-lua/plenary.nvim", -- required
      "esmuellert/codediff.nvim", -- optional - Diff integration
    },
    keys = {
      { "<leader>gg", "<cmd>Neogit<cr>", desc = "Git" },
      {
        "<leader>gl",
        function()
          require("neogit").action("log", "log_current", { "--graph", "--decorate", "--max-count", "100" })()
        end,
      },
      {
        "<leader>gf",
        function()
          local file = vim.fn.expand("%")
          vim.cmd([[execute "normal! \<ESC>"]])
          local line_start = vim.fn.getpos("'<")[2]
          local line_end = vim.fn.getpos("'>")[2]
          require("neogit").action("log", "log_current", { "-L" .. line_start .. "," .. line_end .. ":" .. file })()
        end,
        desc = "Neogit Log for this range",
        mode = "v",
      },
      {
        "<leader>gf",
        function()
          local file = vim.fn.expand("%")
          require("neogit").action("log", "log_current", { "--", file })()
        end,
        desc = "Neogit Log for this file",
      },
    },
    opts = {
      kind = "tab",
      disable_hint = true,
      graph_style = "unicode",
      integrations = {
        codediff = true,
      },
      diff_viewer = "codediff",
      log_view = { kind = "tab" },
    },
  },
  {
    "esmuellert/codediff.nvim",
    dependencies = { "MunifTanjim/nui.nvim" },
    cmd = "CodeDiff",
    opts = {
      explorer = {
        view_mode = "tree",
      },
    },
    keys = {
      { "<leader>dd", "<cmd>CodeDiff<cr>", desc = "Git status diff" },
      {
        "<leader>df",
        function()
          vim.cmd("CodeDiff file HEAD " .. vim.fn.expand("%"))
        end,
        desc = "File diff vs HEAD",
      },
      { "<leader>dm", "<cmd>CodeDiff main...<cr>", desc = "Diff vs main (PR)" },
    },
  },
  {
    "tpope/vim-fugitive",
    cmd = { "G", "Git", "GBrowse" },
    keys = {
      { "<leader>gx", ":GBrowse! upstream/main:%<cr>", mode = { "n", "v" } },
      { "<leader>gX", ":GBrowse upstream/main:%<cr>", mode = { "n", "v" } },
    },
    dependencies = {
      "tpope/vim-rhubarb",
    },
  },
  {
    "lewis6991/gitsigns.nvim",
    opts = {
      on_attach = function(buffer)
        local gs = package.loaded.gitsigns

        local function map(mode, l, r, desc)
          vim.keymap.set(mode, l, r, { buffer = buffer, desc = desc })
        end

        map("n", "]c", function()
          if vim.wo.diff then
            vim.cmd.normal({ "]czz", bang = true })
          else
            gs.nav_hunk("next")
            vim.fn.feedkeys("zz")
          end
        end, "Next Hunk")
        map("n", "[c", function()
          if vim.wo.diff then
            vim.cmd.normal({ "[czz", bang = true })
          else
            gs.nav_hunk("prev")
            vim.fn.feedkeys("zz")
          end
        end, "Prev Hunk")

        -- stylua: ignore start
        map({ "n", "v" }, "<leader>gr", ":Gitsigns reset_hunk<CR>", "Reset Hunk")
        map("n", "<leader>gu", gs.undo_stage_hunk, "Undo Stage Hunk")
        map("n", "<leader>gp", gs.preview_hunk_inline, "Preview Hunk Inline")
        map({ "o", "x" }, "ih", ":<C-U>Gitsigns select_hunk<CR>", "GitSigns Select Hunk")
        map("n", "<leader>ghb", function() gs.blame_line({ full = true }) end, "Blame Line")
        map("n", "<leader>ghB", function() gs.blame() end, "Blame Buffer")
        map("n", "<leader>ghd", gs.diffthis, "Diff This")
        map("n", "<leader>ghD", function() gs.diffthis("~") end, "Diff This ~")
      end,
    },
  },
  {
    "tpope/vim-fugitive",
    cmd = { "G", "Git", "Gbrowse" },
    dependencies = {
      { "tpope/vim-rhubarb" },
    },
    config = function()
      vim.api.nvim_create_autocmd("FileType", {
        pattern = "fugitive",
        callback = function(ctx)
          vim.keymap.set("n", "q", ":q<cr>", { buffer = ctx.buf })
          vim.keymap.set("n", "S", "<cmd>silent !git add -A<cr>", { buffer = ctx.buf, desc = "Add All" })
        end,
      })
    end,
  },
  {
    "linrongbin16/gitlinker.nvim",
    dependencies = "nvim-lua/plenary.nvim",
    opts = {},
    keys = {
      { "<leader>gy", "<cmd>GitLink<cr>", mode = { "n", "v" }, desc = "Yank git link" },
      { "<leader>gY", "<cmd>GitLink!<cr>", mode = { "n", "v" }, desc = "Open git link" },
    },
  },

  -- ai
  {
    "supermaven-inc/supermaven-nvim",
    opts = {
      keymaps = {
        accept_suggestion = "<C-r>",
      },
      ignore_filetypes = { markdown = true, codecompanion = true },
    },
  },

  {
    "christoomey/vim-tmux-navigator",
    cmd = {
      "TmuxNavigateLeft",
      "TmuxNavigateDown",
      "TmuxNavigateUp",
      "TmuxNavigateRight",
      "TmuxNavigatePrevious",
    },
    keys = {
      { "<c-h>", "<cmd><C-U>TmuxNavigateLeft<cr>" },
      { "<c-j>", "<cmd><C-U>TmuxNavigateDown<cr>" },
      { "<c-k>", "<cmd><C-U>TmuxNavigateUp<cr>" },
      { "<c-l>", "<cmd><C-U>TmuxNavigateRight<cr>" },
      { "<c-\\>", "<cmd><C-U>TmuxNavigatePrevious<cr>" },
    },
  },

  -- {
  --   "epwalsh/obsidian.nvim",
  --   version = "*", -- recommended, use latest release instead of latest commit
  --   lazy = true,
  --   ft = "markdown",
  --   dependencies = {
  --     "nvim-lua/plenary.nvim",
  --   },
  --   keys = {
  --     { "<leader>fnw", "<cmd>ObsidianSearch<cr>", "Search Obsidian" },
  --     { "<leader>fnd", "<cmd>ObsidianToday<cr>", "Today's Note" },
  --   },
  --   opts = {
  --     workspaces = {
  --       {
  --         name = "default",
  --         path = "~/.brain",
  --       },
  --     },
  --     completion = {
  --       nvim_cmp = true,
  --       min_chars = 2,
  --     },
  --     picker = {
  --       name = "fzf-lua",
  --     },
  --   },
  -- },

  -- snacks
  {
    "folke/snacks.nvim",
    opts = {
      dashboard = { enabled = false },
      scroll = { enabled = false },
      indent = { enabled = false },
      picker = {
        previewers = {
          git = {
            native = false,
          },
        },
        win = {
          input = {
            keys = {},
          },
          list = {
            keys = {},
          },
          preview = {},
        },
        sources = {
          files = {
            hidden = true,
          },
          grep = {
            hidden = true,
          },
        },
        formatters = {
          file = {
            truncate = 80,
          },
        },
      },
    },
    keys = {
      -- stylua: ignore start
      { "<leader>ff", function() Snacks.picker.files() end, desc = "Find Files" },
      { "<leader>,", function() Snacks.picker.buffers() end, desc = "Buffers" },
      { "<leader>/", function() Snacks.picker.grep() end, desc = "Grep" },
      { "<leader>;", function() Snacks.picker.resume() end, desc = "Resume" },

      { "<leader>fk", function() Snacks.picker.keymaps() end, desc = "Keymaps" },

      { "<leader>fd", function() Snacks.picker.diagnostics_buffer() end, desc = "Buffer Diagnostics" },
      { "<leader>fD", function() Snacks.picker.diagnostics() end, desc = "Diagnostics" },

      -- lsp
      { "gd", function() Snacks.picker.lsp_definitions() end, desc = "Goto Definition" },
      { "gD", function() Snacks.picker.lsp_declarations() end, desc = "Goto Declaration" },
      { "gr", function() Snacks.picker.lsp_references() end, nowait = true, desc = "References" },
      { "gI", function() Snacks.picker.lsp_implementations() end, desc = "Goto Implementation" },
      { "gt", function() Snacks.picker.lsp_type_definitions() end, desc = "Goto T[y]pe Definition" },
      { "<leader>fs", function() Snacks.picker.lsp_symbols() end, desc = "LSP Symbols" },
      { "<leader>fS", function() Snacks.picker.lsp_workspace_symbols() end, desc = "LSP Workspace Symbols" },
      { "<leader>ci", function() Snacks.picker.lsp_incoming_calls() end, desc = "Incoming Calls" },
      { "<leader>co", function() Snacks.picker.lsp_outgoing_calls() end, desc = "Outgoing Calls" },
      { "<leader>cr", vim.lsp.buf.rename, desc = "Rename Symbol" },

      -- git
      { "<leader>gb", function() Snacks.picker.git_branches() end, desc = "Git Branches" },
      { "<leader>gs", function() Snacks.picker.git_status() end, desc = "Git Status" },
      { "<leader>gS", function() Snacks.picker.git_stash() end, desc = "Git Stash" },
      { "<leader>gd", function() Snacks.picker.git_diff() end, desc = "Git Diff (Hunks)" },
      { "<leader>gD", function() Snacks.picker.git_diff({ base = "main" }) end, desc = "Git Diff vs Main (PR)" },
      { "<leader>gL", function() Snacks.picker.git_log_line() end, desc = "Git Log Line" },

      -- github
    { "<leader>ghi", function() Snacks.picker.gh_issue() end, desc = "GitHub Issues (open)" },
    { "<leader>ghI", function() Snacks.picker.gh_issue({ state = "all" }) end, desc = "GitHub Issues (all)" },
    { "<leader>ghp", function() Snacks.picker.gh_pr() end, desc = "GitHub Pull Requests (open)" },
    { "<leader>ghP", function() Snacks.picker.gh_pr({ state = "all" }) end, desc = "GitHub Pull Requests (all)" },

      -- utilities
      { "<leader>fu", function() Snacks.picker.undo() end, desc = "Undo History" },
      { "<leader>fn", function() Snacks.picker.notifications() end, desc = "Notifications" },
    },
  },

  -- disables
  { "nvim-neo-tree/neo-tree.nvim", enabled = false },
  { "akinsho/bufferline.nvim", enabled = false },
  { "rcarriga/nvim-notify", enabled = false },
}
