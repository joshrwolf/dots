local M = {}

local util = require('lspconfig/util')

local lastRootPath = nil
local gopath = os.getenv("GOPATH")
if gopath == nil then
  gopath = ""
end
local gopathmod = gopath..'/pkg/mod'

local servers = {
  gopls = {
    cmd = { "gopls", "-remote=auto" },
    -- root_dir = function (fname)
    --   local fullpath = vim.fn.expand(fname, ':p')
    --   if string.find(fullpath, gopathmod) and lastRootPath ~= nil then
    --     -- print("reused lsp: " .. lastRootPath)
    --     return lastRootPath
    --   end
    --   lastRootPath = util.root_pattern("go.work", "go.mod", ".git")(fname)
    --   -- print("new lsp loaded: " .. lastRootPath)
    --   return lastRootPath
    -- end,
    -- root_dir = function (fname)
    --   -- return util.root_pattern('.git')(fname)
    --   return util.root_pattern('go.work')(fname) or util.root_pattern('go.mod', '.git')(fname)
    -- end,
    -- workspace_folders = { 
    --   {
    --     name = "eots", 
    --     uri = "file:///Users/wolf/dev/gh/chainguard/mono/eots",
    --   },
    --   {
    --     name = "gulfstream",
    --     uri = "file:///Users/wolf/dev/gh/chainguard/mono/gulfstream/api",
    --   },
    -- },
    -- before_init  = function (initialize_params)
    --   -- print(vim.inspect(initialize_params['workspaceFolders']))
    --   initialize_params['workspaceFolders'] = {
    --     {
    --       name = "eots",
    --       uri = "file:///Users/wolf/dev/gh/chainguard/mono/eots",
    --     },
    --     {
    --       name = "gulfstream-api",
    --       uri = "file:///Users/wolf/dev/gh/chainguard/mono/gulfstream/api",
    --     },
    --   }
    --   print(vim.inspect(initialize_params))
    -- end
  },
  rust_analyzer = {},
  html = {},
  jsonls = {
    settings = {
      json = {
        schemas = require("schemastore").json.schemas()
      }
    }
  },
  pyright = {},
  sumneko_lua = {
    settings = {
      Lua = {
        runtime = {
          version = "LuaJIT",
          path = vim.split(package.path, ";"),
        },
        diagnostics = {
          globals = { "vim", "describe", "it", "before_each", "after_each", "packer_plugins" },
          disable = { "lowercase-global", "undefined-global", "unused-local", "unused-vararg", "trailing-space" },
        },
        workspace = {
          library = {
            [vim.fn.expand "$VIMRUNTIME/lua"] = true,
            [vim.fn.expand "VIMRUNTIME/lua/vim/lsp"] = true,
          },
        },
        completion = { callSnippet = "Both" },
        telemetry = { enable = false },
      },
    },
  },
  terraformls = {},
  tsserver = {},
  vimls = {},
  dockerls = {},
  bashls = {},
  yamlls = {
    schemas = {
      kubernetes = "*.yaml",
      ["http://json.schemastore.org/github-workflow"] = ".github/workflows/*",
      ["http://json.schemastore.org/github-action"] = ".github/action.{yml,yaml}",
      ["http://json.schemastore.org/kustomization"] = "kustomization.{yml,yaml}",
      ["https://json.schemastore.org/dependabot-v2"] = ".github/dependabot.{yml,yaml}",
      ["https://json.schemastore.org/gitlab-ci"] = "*gitlab-ci*.{yml,yaml}",
      ["https://raw.githubusercontent.com/OAI/OpenAPI-Specification/main/schemas/v3.1/schema.json"] = "*api*.{yml,yaml}",
      ["https://raw.githubusercontent.com/compose-spec/compose-spec/master/schema/compose-spec.json"] = "*docker-compose*.{yml,yaml}",
      ["https://raw.githubusercontent.com/argoproj/argo-workflows/master/api/jsonschema/schema.json"] = "*flow*.{yml,yaml}",
    },
  },
}

function M.on_attach(client, bufnr)
  -- Enable completion triggered by <C-X><C-O>
  -- See `:help omnifunc` and `:help ins-completion` for more information.
  vim.api.nvim_buf_set_option(bufnr, "omnifunc", "v:lua.vim.lsp.omnifunc")

  -- Use LSP as the handler for formatexpr.
  -- See `:help formatexpr` for more information.
  vim.api.nvim_buf_set_option(0, "formatexpr", "v:lua.vim.lsp.formatexpr()")

  vim.lsp.handlers['textDocument/hover'] = vim.lsp.with(
    vim.lsp.handlers.hover,
    { border = 'rounded' }
  )

  vim.lsp.handlers['textDocument/signatureHelp'] = vim.lsp.with(
    vim.lsp.handlers.signature_help,
    { border = 'rounded' }
  )

  -- Configure LSP specific keymappings
  require("config.lsp.keymaps").setup(client, bufnr)
end

local capabilities = vim.lsp.protocol.make_client_capabilities()
-- capabilities.textDocument.completion.completionItem.snippetSupport = true
M.capabilities = require("cmp_nvim_lsp").update_capabilities(capabilities)

local opts = {
  on_attach = M.on_attach,
  capabilities = M.capabilities,
  flags = {
    debounce_text_changes = 150,
  },
}

function M.setup()
  -- Installer
  require("config.lsp.installer").setup(servers, opts)
end

return M
