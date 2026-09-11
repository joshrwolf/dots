-- Parser versions must follow the queries shipped with nvim-treesitter.
vim.api.nvim_create_autocmd("PackChanged", {
  group = vim.api.nvim_create_augroup("dots.parser_updates", { clear = true }),
  callback = function(event)
    if event.data.spec.name == "nvim-treesitter" and event.data.kind == "update" then
      vim.cmd.packadd("nvim-treesitter")
      require("nvim-treesitter").update()
    end
  end,
})

vim.pack.add({
  "https://github.com/projekt0n/github-nvim-theme",
  "https://github.com/nvim-mini/mini.icons",
  "https://github.com/stevearc/oil.nvim",
  "https://github.com/folke/snacks.nvim",
  "https://github.com/folke/which-key.nvim",
  "https://github.com/neovim/nvim-lspconfig",
  "https://github.com/folke/lazydev.nvim",
  "https://github.com/stevearc/conform.nvim",
  "https://github.com/lewis6991/gitsigns.nvim",
  "https://github.com/esmuellert/codediff.nvim",
  { src = "https://github.com/saghen/blink.cmp", version = "v1.10.2" },
  { src = "https://github.com/nvim-treesitter/nvim-treesitter", version = "main" },
})

-- theme.lua links through the shared selector used by Ghostty.
-- Apply after packages are available and before setting up UI plugins.
dofile(vim.fn.stdpath("config") .. "/theme.lua")

-- Shared icon provider, detected directly by Snacks, Oil, and which-key.
require("mini.icons").setup()

-- Git signs feed Snacks' gutter; defaults keep blame and word-diff overlays off.
require("gitsigns").setup({
  on_attach = function(buf)
    local gs = require("gitsigns")
    vim.keymap.set("n", "]h", function()
      gs.nav_hunk("next")
    end, { buffer = buf, desc = "Next Git hunk" })
    vim.keymap.set("n", "[h", function()
      gs.nav_hunk("prev")
    end, { buffer = buf, desc = "Previous Git hunk" })
    vim.keymap.set("n", "<leader>gp", gs.preview_hunk_inline, { buffer = buf, desc = "Preview Git hunk inline" })
    vim.keymap.set("n", "<leader>gl", gs.blame_line, { buffer = buf, desc = "Blame line" })
  end,
})

-- Full reviews use ordinary split windows. Colors come from the shared theme;
-- stronger text highlights distinguish changed characters within tinted lines.
require("codediff").setup({
  diff = {
    filler_text = " ", -- Quiet shaded gaps instead of diagonal stripes.
  },
  explorer = {
    position = "left", -- The right-sidebar workaround retains upstream's width sizing.
    view_mode = "tree",
  },
  highlights = {
    char_insert = "DiffTextAdd",
    char_delete = "DiffText",
  },
  keymaps = {
    view = {
      next_hunk = "]h",
      prev_hunk = "[h",
      toggle_explorer = "<leader>ge", -- Keep the buffer prefix available.
    },
  },
})

require("config.codediff-sidebar")

-- Discover mappings from their desc fields; only group names belong here.
-- The default 200ms delay shows hints after pausing on a prefix such as Space.
require("which-key").setup({
  spec = {
    { "<leader>f", group = "Find" },
    { "<leader>c", group = "Code" },
    { "<leader>b", group = "Buffers" },
    { "<leader>g", group = "Git", mode = { "n", "x" } },
  },
})

-- Add Neovim/plugin types to LuaLS as needed; works with native packages.
require("lazydev").setup({
  library = {
    { path = "snacks.nvim", words = { "Snacks" } },
  },
  -- Respect Lua projects that supply their own workspace configuration.
  enabled = function(root)
    return not (vim.uv.fs_stat(root .. "/.luarc.json") or vim.uv.fs_stat(root .. "/.luarc.jsonc"))
  end,
})

-- Blink owns completion; native snippets need no separate snippet engine.
-- Ctrl+N/P selects, Ctrl+Y accepts, Ctrl+Space opens completion/docs.
require("blink.cmp").setup({
  keymap = { preset = "default", ["<C-k>"] = false }, -- Reserve pane navigation.
  completion = { documentation = { auto_show = true } },
  sources = {
    default = { "lazydev", "lsp", "path", "snippets", "buffer" },
    providers = {
      lazydev = { name = "LazyDev", module = "lazydev.integrations.blink", score_offset = 100 },
    },
  },
})

require("config.treesitter")

-- Conform owns formatting, so saves never run both an external and LSP formatter.
-- Tools come from Homebrew, except rustfmt, which follows the project's rustup toolchain.
require("conform").setup({
  formatters_by_ft = {
    lua = { "stylua" },
    rust = { "rustfmt" },
    go = { "goimports" },
  },
  format_on_save = { timeout_ms = 2000, lsp_format = "never" },
})

-- Edit directories as buffers; :write applies changes after confirmation.
require("oil").setup({
  view_options = { show_hidden = true }, -- Toggle with g.
  keymaps = {
    -- Keep Herdr's prefix and pane navigation keys free.
    ["<C-s>"] = false,
    ["<C-h>"] = false,
    ["<C-l>"] = false,
    ["<C-v>"] = { "actions.select", opts = { vertical = true } },
    ["<C-x>"] = { "actions.select", opts = { horizontal = true } },
    ["gR"] = "actions.refresh",
  },
})

-- Enable individual Snacks features explicitly.
require("snacks").setup({
  bigfile = { enabled = true },
  indent = { enabled = true, animate = { enabled = false } },
  input = { enabled = true },
  notifier = { enabled = true },
  statuscolumn = { enabled = true },
  words = { enabled = true },
  picker = {
    enabled = true,
    -- Ctrl+X replaces Ctrl+S (Herdr's prefix); Ctrl+V keeps its vertical split default.
    win = {
      input = {
        keys = {
          ["<c-s>"] = false,
          ["<c-x>"] = { "edit_split", mode = { "i", "n" } },
        },
      },
      list = {
        keys = {
          ["<c-s>"] = false,
          ["<c-x>"] = "edit_split",
        },
      },
    },
    -- Search from Neovim's cwd, including dotfiles but respecting ignore files.
    sources = {
      files = { cmd = "rg", hidden = true },
      grep = { hidden = true },
    },
  },
})
