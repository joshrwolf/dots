-- nvim-lspconfig supplies server commands, filetypes, and project-root detection.
-- Binaries live on PATH; Rust setup: rustup component add rust-analyzer rust-src clippy rustfmt

-- Snacks' simple LSP progress recipe: update one notification as servers work.
-- https://github.com/folke/snacks.nvim/blob/main/docs/notifier.md
vim.api.nvim_create_autocmd("LspProgress", {
  group = vim.api.nvim_create_augroup("dots.lsp_progress", { clear = true }),
  callback = function(event)
    local spinner = { "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏" }
    vim.notify(vim.lsp.status(), vim.log.levels.INFO, {
      id = "lsp_progress",
      title = "LSP Progress",
      opts = function(notification)
        notification.icon = event.data.params.value.kind == "end" and " "
          or spinner[math.floor(vim.uv.hrtime() / (1e6 * 80)) % #spinner + 1]
      end,
    })
  end,
})

vim.lsp.config("rust_analyzer", {
  settings = {
    ["rust-analyzer"] = { check = { command = "clippy" } },
  },
})

-- Semantic tokens add type-aware Go colors on top of Tree-sitter's syntax.
vim.lsp.config("gopls", {
  settings = { gopls = { semanticTokens = true } },
})

vim.api.nvim_create_autocmd("LspAttach", {
  group = vim.api.nvim_create_augroup("dots.lsp", { clear = true }),
  callback = function(event)
    vim.keymap.set("n", "gd", vim.lsp.buf.definition, { buffer = event.buf, desc = "Go to definition" })
    -- K, grn, gra, grr, gri, grt, and gO use Neovim's built-in LSP mappings.
    -- Signature help gets a reachable key because Herdr owns Ctrl+S.
    vim.keymap.set("n", "<leader>cs", vim.lsp.buf.signature_help, {
      buffer = event.buf,
      desc = "Signature help",
    })
  end,
})

-- LuaLS and gopls provide diagnostics directly; rust-analyzer also runs Clippy.
vim.lsp.enable({ "lua_ls", "rust_analyzer", "gopls" })
