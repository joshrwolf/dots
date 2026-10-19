-- Install missing parsers only; updates happen with the plugin (or :TSUpdate).
-- Keep the list explicit: our languages, config files, and documentation.
require("nvim-treesitter").install({
  "lua",
  "rust",
  "go",
  "gomod",
  "gosum",
  "gowork",
  "bash",
  "json",
  "toml",
  "yaml",
  "markdown",
  "markdown_inline",
  "vim",
  "vimdoc",
  "query",
})

vim.api.nvim_create_autocmd("FileType", {
  group = vim.api.nvim_create_augroup("dots.treesitter", { clear = true }),
  callback = function(event)
    if vim.bo[event.buf].filetype == "bigfile" then
      return
    end
    -- Unsupported filetypes keep regular syntax; fresh installs may still be building.
    pcall(vim.treesitter.start, event.buf)
  end,
})
