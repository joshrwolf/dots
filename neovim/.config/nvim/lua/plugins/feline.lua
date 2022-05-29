require('feline').setup({
    components = {
        active = {
            -- Left
            {
                vimode = {
                    provider = function()
                        return string.format(" %s ", "hi")
                    end,
                },
                gitbranch = {},
                file_type = {
                    provider = function()
                        return fmt(" %s ", vim.bo.filetype:upper())
                    end,
                    hl = "F1nAlt",
                },
            },

            -- Right
            {
            },
        },
        inactive = {},
    },
    force_inactive = {
        filetypes = {
            "NvimTree",
            "packer",
            "dap-repl",
            "dapui_scopes",
            "dapui_stacks",
            "dapui_watches",
            "dapui_repl",
            "LspTrouble",
            "qf",
            "help",
        },
    },
})
