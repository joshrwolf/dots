local M = {}

function M.setup(client, buf)
	local nls = require("null-ls")
	local augroup = vim.api.nvim_create_augroup("LspFormatting", {})

	local fmt = nls.builtins.formatting
	local dgn = nls.builtins.diagnostics

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

			-- Linters

			-- Diagnostics
			dgn.actionlint,
		},
	})
end

return M
