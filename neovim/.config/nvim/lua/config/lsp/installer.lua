local M = {}

function M.setup(servers, options)
  local lspconfig = require "lspconfig"

  require("nvim-lsp-installer").setup {
    automatic_installation = true,
  }

  for server_name, _ in pairs(servers) do
    local opts = vim.tbl_deep_extend("force", options, servers[server_name] or {})

    if server_name == "sumneko_lua" then
      lspconfig[server_name].setup(opts)
    else
      lspconfig[server_name].setup(opts)
    end

  end
end

return M
