local M = {}

function M.setup(client, buf)
	local nls = require("null-ls")
	local augroup = vim.api.nvim_create_augroup("LspFormatting", {})

	local fmt = nls.builtins.formatting
	local dgn = nls.builtins.diagnostics
	local ca = nls.builtins.code_actions

	nls.setup({
		debounce = 200,
		on_attach = function(client, bufnr)
			if client.supports_method("textDocument/formatting") then
				vim.api.nvim_clear_autocmds({ group = augroup, buffer = bufnr })
				vim.api.nvim_create_autocmd("BufWritePre", {
					group = augroup,
					buffer = bufnr,
					callback = function()
						vim.lsp.buf.format({
							bufnr = bufnr,
							filter = function(client)
								return client.name == "null-ls"
							end,
						})
					end,
				})
			end
		end,
		sources = {
			-- Formatters
			fmt.stylua,
			fmt.terraform_fmt,

			fmt.goimports,
			fmt.gofumpt,

			fmt.rustfmt,

			fmt.trim_whitespace,
			fmt.trim_newlines,

			-- Code Actions
			ca.shellcheck,
			ca.gitrebase,
			-- ca.gomodifytags,

			-- Diagnostics
			dgn.actionlint,
			dgn.buf,
			-- dgn.golangci_lint, -- NOTE: This doesn't work in mono because null-ls always attaches to the folder where .git is
			dgn.vale,
		},
	})
end

return M
