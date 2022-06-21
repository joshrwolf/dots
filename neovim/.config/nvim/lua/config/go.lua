local M = {}

function M.setup()
  require("go").setup({
    lsp_cfg = true,
    lsp_gofumpt = false,
    lsp_on_attach = function(client, bufnr)
      require("config.lsp").on_attach(client, bufnr)
    end,
    dap_debug = false,
    build_tags = "",
  })
end

return M
