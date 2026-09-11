-- Match Ghostty's GitHub Light Default with the plugin's corresponding variant.
vim.opt.termguicolors = true
require("github-theme").setup({
  groups = {
    github_light_default = {
      -- Shared diff colors for native diffs, Snacks, Gitsigns, and CodeDiff.
      -- Background-only highlights preserve the renderer's syntax colors.
      DiffAdd = { bg = "#e6ffec" },
      DiffDelete = { bg = "#ffebe9" },
      DiffChange = { bg = "#ddf4ff" },
      -- Soft opaque tints approximate translucent overlays on the light background.
      DiffText = { fg = "NONE", bg = "#f8d4d0" },
      DiffTextAdd = { bg = "#ccebd4" },
      CodeDiffFiller = { bg = "#f6f8fa" },
      GitSignsAddPreview = { link = "DiffAdd" },
      GitSignsDeletePreview = { link = "DiffDelete" },
      GitSignsDeleteVirtLn = { link = "DiffDelete" },
      GitSignsAddInline = { link = "DiffTextAdd" },
      GitSignsChangeInline = { link = "DiffTextAdd" },
      GitSignsDeleteInline = { link = "DiffText" },
    },
  },
})
vim.cmd.colorscheme("github_light_default")
