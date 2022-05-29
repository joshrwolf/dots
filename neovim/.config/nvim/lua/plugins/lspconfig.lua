require('nvim-lsp-installer').setup {
  automatic_installation = true,
}

local lspconfig = require('lspconfig')

local capabilities = vim.lsp.protocol.make_client_capabilities()
capabilities = require('cmp_nvim_lsp').update_capabilities(capabilities)

local function on_attach(_, bufnr)
  local opts = { buffer = bufnr }

  vim.keymap.set('n', 'gd', vim.lsp.buf.definition, opts)
  vim.keymap.set('n', 'gD', vim.lsp.buf.declaration, opts)
  vim.keymap.set('n', 'K', vim.lsp.buf.hover, opts)
  vim.keymap.set('n', 'gi', vim.lsp.buf.implementation, opts)
end

-- lua
local runtime_path = vim.split(package.path, ';')
table.insert(runtime_path, 'lua/?.lua')
table.insert(runtime_path, 'lua/?/init.lua')

lspconfig.sumneko_lua.setup {
  on_attach = on_attach,
  capabilities = capabilities,
  settings = {
    Lua = {
      runtime = {
          version = 'LuaJIT',
          path = runtime_path,
      },
      diagnostics = {
          -- Recognize the 'vim' global variable
          globals = { 'vim' },
      },
      workspace = {
          -- Make the server aware of nvim runtime files
          -- library = vim.api.nvim_get_runtime_file('', true),
          library = {
              [vim.fn.expand('$VIMRUNTIME/lua')] = true,
              [vim.fn.expand('$VIMRUNTIME/lua/vim/lsp')] = true,
              [vim.fn.stdpath('config') .. '/lua'] = true,
          },
          maxPreload = 10000,
      },
      telemetry = {
          enable = false,
      },
    },
  },
}

-- go
lspconfig.gopls.setup {
  on_attach = on_attach,
  capabilities = capabilities,
  cmd = { 'gopls' },
  filetypes = { 'go', 'gomod' },
  settings = {
    gopls = {
      analyses = {
        unusedparams = true,
      },
      staticcheck = true,
      codelenses = {
        generate = true,
        test = true,
        tidy = true,
      },
      build = {
        experimentalWorkspaceModule = true,
      },
    },
  },
}

-- yamlls
-- reference: https://github.com/redhat-developer/yaml-language-server
lspconfig.yamlls.setup {
  on_attach = on_attach,
  capabilities = capabilities,
  settings = {
    yaml = {
      schemas = {
        ['kubernetes'] = '/*.yaml',
      },
    },
  },
}

-- terraformls
lspconfig.terraformls.setup {
  on_attach = on_attach,
  capabilities = capabilities,
}

-- javascript/typescript
lspconfig.tsserver.setup {
  on_attach = on_attach,
  capabilities = capabilities,
}

-- local lsp_installer = require('nvim-lsp-installer')
--
-- -- TODO: Make this only run when lua is detected?
-- local runtime_path = vim.split(package.path, ';')
-- table.insert(runtime_path, 'lua/?.lua')
-- table.insert(runtime_path, 'lua/?/init.lua')
--
-- local servers = {
--     sumneko_lua = {
--         runtime = {
--             version = 'LuaJIT',
--             path = runtime_path,
--         },
--         diagnostics = {
--             -- Recognize the 'vim' global variable
--             globals = { 'vim' },
--         },
--         workspace = {
--             -- Make the server aware of nvim runtime files
--             -- library = vim.api.nvim_get_runtime_file('', true),
--             library = {
--                 [vim.fn.expand('$VIMRUNTIME/lua')] = true,
--                 [vim.fn.expand('$VIMRUNTIME/lua/vim/lsp')] = true,
--                 [vim.fn.stdpath('config') .. '/lua'] = true,
--             },
--             maxPreload = 10000,
--         },
--         telemetry = {
--             enable = false,
--         },
--     },
--     gopls = {
--         experimentalPostfixCompletions = true,
--         analyses = {
--             unusedparams = true,
--             shadow = true,
--         },
--         staticcheck = true,
--     },
--     terraformls = {},
--     yamlls = {},
-- }
--
-- local on_attach = function(client, bufnr)
--     local opts = { buffer = bufnr }
--
--     -- require('aerial').on_attach()
--
--     -- Enable completion triggered by <C-X><C-O>
--     vim.api.nvim_buf_set_option(bufnr, 'omnifunc', 'v:lua.vim.lsp.omnifunc')
--
--     -- Use LSP as the handler for formatexpr
--     vim.api.nvim_buf_set_option(0, 'formatexpr', 'v:lua.vim.lsp.formatexpr()')
--
--     vim.keymap.set('n', 'gD', vim.lsp.buf.declaration, opts)
--     vim.keymap.set('n', 'gd', vim.lsp.buf.definition, opts)
--     vim.keymap.set('n', 'K', vim.lsp.buf.hover, opts)
--     vim.keymap.set('n', 'gi', vim.lsp.buf.implementation, opts)
--
--     vim.api.nvim_create_user_command('Format', vim.lsp.buf.formatting, {})
-- end
--
-- -- Enable the following language servers
-- for name, _ in pairs(servers) do
--     local found, server = lsp_installer.get_server(name)
--
--     if found and not server:is_installed() then
--         print('Installing ' .. name)
--         server:install()
--     end
-- end
--
-- -- Register handler for setting server up when they're ready (installed or loaded)
-- lsp_installer.on_server_ready(function(server)
--     local caps = require('cmp_nvim_lsp').update_capabilities(vim.lsp.protocol.make_client_capabilities())
--
--     local opts = {
--         on_attach = on_attach,
--         capabilities = caps,
--     }
--
--     if server.name == 'sumneko_lua' then
--         opts.settings = { Lua = servers.sumneko_lua }
--     elseif server.name == 'gopls' then
--       opts.settings = { gopls = servers.gopls }
--     end
--
--     server:setup(opts)
-- end)
--
--

require('lsp_signature').setup({
  transparency = 10,
})
