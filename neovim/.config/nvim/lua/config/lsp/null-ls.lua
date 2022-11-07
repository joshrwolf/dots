local M = {}

local nls = require("null-ls")
local augroup = vim.api.nvim_create_augroup("LspFormatting", {})

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
			-- fmt.yamlfmt,

			-- Diagnostics
			dgn.shellcheck,
			dgn.actionlint,
			-- dgn.cue_fmt,		-- TODO: This doesn't work :(
			dgn.buf,
			dgn.markdownlint,
			-- dgn.semgrep,
			dgn.cfn_lint,
			-- dgn.yamllint,

			-- Code Actions
			cda.shellcheck,
			cda.gitsigns,
			cda.gitrebase,
		},
		on_attach = function(client, bufnr)
			if client.supports_method("textDocument/formatting") then
				vim.api.nvim_clear_autocmds({ group = augroup, buffer = bufnr })
				vim.api.nvim_create_autocmd("BufWritePre", {
					group = augroup,
					buffer = bufnr,
					callback = function()
						vim.lsp.buf.format({
							bufnr = bufnr,

							-- Only let null-ls do the formatting, lsp's suck amirite
							filter = function(client)
								return client.name == "null-ls"
							end,
						})
					end,
				})
			end
		end,
	})
end

return M
