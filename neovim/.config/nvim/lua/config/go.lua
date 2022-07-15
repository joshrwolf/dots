local M = {}

local util = require 'lspconfig/util'

function M.setup()
  require("go").setup({
    lsp_cfg = {
      settings = {
        experimentalWorkspaceModule = true,
        gopls = {
          usePlaceholders = false,
        },
      },
      root_dir = function(fname)
        return util.root_pattern 'go.work'(fname) or util.root_pattern('go.mod', '.git')(fname)
        -- return util.root_pattern('go.mod', '.git')(fname) or dirname(fname) -- util.path.dirname(fname)
      end,
    },
    lsp_gofumpt = false,
    lsp_on_attach = function(client, bufnr)
      require("config.lsp").on_attach(client, bufnr)
    end,
    dap_debug = false,
    build_tags = "",
  })
end

return M
