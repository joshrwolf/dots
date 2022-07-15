local M = {}

function M.setup(servers, options)
  local lspconfig = require "lspconfig"

  require("nvim-lsp-installer").setup {
    automatic_installation = true,
  }

  -- lsp config as json
  require("nlspsettings").setup({
    config_home = vim.fn.stdpath('config') .. '/nlsp-settings',
    local_settings_dir = ".nlsp-settings",
    local_settings_root_markers = { '.git' },
    append_default_schemas = true,
    loader = 'json',
  })

  for server_name, _ in pairs(servers) do
    local opts = vim.tbl_deep_extend("force", options, servers[server_name] or {})

    if server_name == "sumneko_lua" then
      local luadev = require("lua-dev").setup { lspconfig = opts }
      lspconfig[server_name].setup(luadev)
    else
      lspconfig[server_name].setup(opts)
    end

  end
end

return M
