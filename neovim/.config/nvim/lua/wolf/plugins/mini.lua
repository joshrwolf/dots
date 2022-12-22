local M = {
    "echasnovski/mini.nvim",
    dependencies = {
        { "JoosepAlviste/nvim-ts-context-commentstring" },
    }
}

function M.pairs()
    require("mini.pairs").setup({})
end

function M.comment()
    require("mini.comment").setup({
        hooks = {
            pre = function()
                require("ts_context_commentstring.internal").update_commentstring({})
            end
        },
    })
end

function M.init()
    M.pairs()
    M.comment()
end

return M
