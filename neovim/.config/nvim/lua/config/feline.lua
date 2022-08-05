local M = {}
local status_ok, feline = pcall(require, "feline")
if not status_ok then
	return
end
local hl = require("core.status").hl
local provider = require("core.status").provider
local conditional = require("core.status").conditional

-- stylua: ignore
local components = {
  active = {
    {
      { provider = provider.spacer(), hl = hl.mode() },
      { provider = provider.spacer(2) },
      { provider = "git_branch", icon = " " },
      { provider = provider.spacer(3), enabled = conditional.git_available },
      { provider = "file_info", opts = { type = "unique-short", colored_icon = false },
        enabled = conditional.has_filetype },
      { provider = provider.spacer(2), enabled = conditional.has_filetype },
      { provider = "git_diff_added", hl = hl.fg("GitSignsAdd"), icon = "  " },
      { provider = "git_diff_changed", hl = hl.fg("GitSignsChange"), icon = " 柳" },
      { provider = "git_diff_removed", hl = hl.fg("GitSignsDelete"), icon = "  " },
      { provider = provider.spacer(2), enabled = conditional.git_changed },
      { provider = "diagnostic_errors", hl = hl.fg("DiagnosticError"), icon = "  " },
      { provider = "diagnostic_warnings", hl = hl.fg("DiagnosticWarn"), icon = "  " },
      { provider = "diagnostic_info", hl = hl.fg("DiagnosticInfo"), icon = "  " },
      { provider = "diagnostic_hints", hl = hl.fg("DiagnosticHint"), icon = "  " },
    },
    {
      { provider = provider.lsp_client_names(true), short_provider = provider.lsp_client_names(),
        enabled = conditional.bar_width(), icon = "   " },
      { provider = provider.spacer(2), enabled = conditional.has_filetype },
      { provider = { name = "file_type", opts = { filetype_icon = true, case = "lowercase" } },
        enabled = conditional.has_filetype },
      { provider = provider.spacer(2), enabled = conditional.bar_width() },
      { provider = provider.treesitter_status, enabled = conditional.bar_width(), hl = hl.fg("GitSignsAdd") },
      { provider = provider.spacer(2) },
      { provider = "position" },
      { provider = provider.spacer(2) },
      { provider = "line_percentage" },
      { provider = provider.spacer() },
      { provider = "scroll_bar", hl = hl.fg("TypeDef") },
      { provider = provider.spacer(2) },
      { provider = provider.spacer(), hl = hl.mode() },
    },
  },
}

function M.setup()
	require("feline").setup({
		disable = { filetypes = { "^NvimTree$", "^neo%-tree$", "^dashboard$", "^Outline$", "^aerial$" } },
		theme = hl.group("StatusLine"),
		components = components,
	})
end

return M
