local vo = vim.opt_local

vo.tabstop = 4
vo.shiftwidth = 4
vo.softtabstop = 4

local wk = require("which-key")

wk.register({
  c = {
    name = "Code",
    i = { "<cmd>GoImport<cr>", "Go Import" },
    I = { "<cmd>GoImpl<cr>", "Go Implement" },
    f = { "<cmd>GoFillStruct<cr>", "Go Fill Struct" },
  }
}, { prefix = "<leader>", mode = "n", silent = true })

function goimports(wait_ms)
  local params = vim.lsp.util.make_range_params()
   params.context = {only = {"source.organizeImports"}}
   local result = vim.lsp.buf_request_sync(0, "textDocument/codeAction", params, wait_ms)
   for _, res in pairs(result or {}) do
     for _, r in pairs(res.result or {}) do
       if r.edit then
         -- note: text encoding param is required
         vim.lsp.util.apply_workspace_edit(r.edit, "utf-16")
       else
         vim.lsp.buf.execute_command(r.command)
       end
     end
   end
end

-- Autoimport and format on *.go file saves
vim.api.nvim_create_autocmd("BufWritePre", {
  pattern = { "*.go" },
  callback = vim.lsp.buf.formatting,
})
--
-- vim.api.nvim_create_autocmd("BufWritePre", {
--   pattern = { "*.go" },
--   callback = goimports(1000)
-- })
--
vim.api.nvim_create_autocmd("BufWritePre", {
  pattern = { "*.go" },
  callback = function ()
    local clients = vim.lsp.buf_get_clients()
    for _, client in pairs(clients) do
      local params = vim.lsp.util.make_range_params(nil, client.offset_encoding)
      params.context = { only = { "source.organizeImports" } }

      local result = vim.lsp.buf_request_sync(0, "textDocument/codeAction", params, 1000)
      for _, res in pairs(result or {}) do
        for _, r in pairs(res.result or {}) do
          if r.edit then
            vim.lsp.util.apply_workspace_edit(r.edit, client.offset_encoding)
          else
            vim.lsp.buf.execute_command(r.command)
          end
        end
      end
    end
  end
})

  -- " autocmd BufWritePre *.go :silent! lua vim.lsp.buf.formatting()
-- vim.cmd[[
-- augroup GO_LSP
--   autocmd!
--   autocmd BufWritePre *.go :silent! lua goimports(3000)
--  augroup END
-- ]]
