local bo = vim.bo
local group = vim.api.nvim_create_augroup("Markdown Wrap Settings", { clear = true })

bo.textwidth = 80

vim.api.nvim_create_autocmd('BufEnter', {
  pattern = { '*.md' },
  group = group,
  command = 'setlocal wrap',
})
