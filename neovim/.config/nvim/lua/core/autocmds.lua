local augroup = vim.api.nvim_create_augroup     -- Create/get autocommand group
local autocmd = vim.api.nvim_create_autocmd     -- Create autocommand

-- Highlight on yank
augroup('YankHighlight', { clear = true })
autocmd('TextYankPost', {
    group = 'YankHighlight',
    callback = function()
        vim.highlight.on_yank({ higroup = 'IncSearch', timeout = '250' })
    end
})

-- Don't auto comment new lines
autocmd('BufEnter', {
    pattern = '*',
    command = 'set fo-=c fo-=r fo-=o'
})

-- Close nvim if NvimTree is the only remaining buffer
autocmd('BufEnter', {
    command = [[if winnr('$') == 1 && bufname() == 'NvimTree_' . tabpagenr() | quit | endif ]]
})

