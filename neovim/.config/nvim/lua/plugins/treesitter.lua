local status_ok, nvim_treesitter = pcall(require, 'nvim-treesitter.configs')
if not status_ok then
    return
end

-- See: https://github.com/nvim-treesitter/nvim-treesitter#quickstart
nvim_treesitter.setup {
    -- A list of parser names, or "all"
    ensure_installed = {
        'bash',
        'lua',
        'vim',
        'yaml',
        'toml',
        'perl',
        'python',
        'nix',
        'rust',
        'rego',
        'norg',
        'latex',
        'json',
        'javascript',
        'html',
        'bash',
        'cmake',
        'dockerfile',
        'go', 'gowork', 'gomod',
        'hcl',
    },

    -- Install parsers synchronously
    sync_installed = false,
    highlight = {
        enable = true,
    },

    -- Text objects
    textobjects = {
      select = {
        enable = true,
        lookahead = true,
        keymaps = {
          ["af"] = "@function.outer",
          ["if"] = "@function.inner",
          ["ac"] = "@class.outer",
          ["ic"] = "@class.inner",
          ["ia"] = "@parameter.inner",
          ["aa"] = "@parameter.outer",
        },
      },
      move = {
        enable = true,
        set_jumps = true,
        goto_next_start = {
          ["]m"] = "@function.outer",
          ["]]"] = "@class.outer",
        },
      },
    },
}

