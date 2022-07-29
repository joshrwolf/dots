local M = {}
local U = require("config.lsp.utils")

local nls = require("null-ls")

local fmt = nls.builtins.formatting
local dgn = nls.builtins.diagnostics
local cda = nls.builtins.code_actions
local cmp = nls.builtins.completion

function M.setup()
	nls.setup({
		sources = {
			-- Completions
			cmp.luasnip,

			-- Formatting
			fmt.stylua,
			fmt.rustfmt,
			fmt.gofmt,
			fmt.goimports,
			fmt.terraform_fmt,

			-- Diagnostics
			dgn.shellcheck,
			dgn.actionlint,
			dgn.cue_fmt,
			dgn.buf,
			dgn.markdownlint,
			-- dgn.semgrep,

			-- Code Actions
			cda.shellcheck,
		},
		on_attach = function(client, bufnr)
			U.format_on_save(client, bufnr)
		end,
	})
end

return M
