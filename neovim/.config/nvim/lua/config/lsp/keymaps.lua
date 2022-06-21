local M = {}

local whichkey = require "which-key"

local function keymappings(client, bufnr)
  local opts = { noremap = true, silent = true }

  -- General keymappings
  vim.api.nvim_buf_set_keymap(bufnr, "n", "K", "<cmd>lua vim.lsp.buf.hover()<CR>", opts)

  vim.api.nvim_set_keymap("n", "[d", "<cmd>lua vim.diagnostic.goto_prev()<CR>", opts)
  vim.api.nvim_set_keymap("n", "]d", "<cmd>lua vim.diagnostic.goto_next()<CR>", opts)
  vim.api.nvim_set_keymap("n", "[e", "<cmd>lua vim.diagnostic.goto_prev({ severity = vim.diagnostic.severity.ERROR })<CR>", opts)
  vim.api.nvim_set_keymap("n", "]d", "<cmd>lua vim.diagnostic.goto_next({ severity = vim.diagnostic.severity.ERROR })<CR>", opts)

  -- Register G based keys
  whichkey.register({
    name = "Goto",
    d = { vim.lsp.buf.definition, "Definition" },
    D = { vim.lsp.buf.declaration, "Declaration" },
    i = { vim.lsp.buf.implementation, "Implementation" },
    r = { vim.lsp.buf.type_definition, "Type Definition" },
  }, { buffer = bufnr, prefix = "g" })

  -- Register LEADER based keys
  whichkey.register({
    l = {
      name = "LSP",
      a = { vim.lsp.buf.code_action, "Code Action" },
      r = { vim.lsp.buf.rename, "Rename" },
      d = { "<cmd>Telescope diagnostics<CR>", "Diagnostics" },
      s = { "<cmd>Telescope lsp_document_symbols<cr>", "Document Symbols" },
      i = { "<cmd>LspInfo<CR>", "Lsp Info" },
      I = { "<cmd>LspInstallInfo<CR>", "Lsp Installer Info" },
      w = {
        name = "Workspaces",
        a = { vim.lsp.buf.add_workspace_folder, "Add Workspace Folder" },
        l = { "<cmd>lua print(vim.inspect(vim.lsp.buf.list_workspace_folders()))<cr>", "List Workspace Folders" },
        r = { vim.lsp.buf.remove_workspace_folder, "Remove Workspace Folder" },
        s = { "<cmd>Telescope lsp_dynamic_workspace_symbols<cr>", "Workspace Symbols" },
      },
    },
  }, { buffer = bufnr, prefix = "<leader>" })
end

function M.setup(client, bufnr)
  keymappings(client, bufnr)
end

return M
