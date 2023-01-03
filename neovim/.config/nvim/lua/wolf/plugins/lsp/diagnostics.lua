local M = {}

M.signs = { Error = " ", Warn = " ", Hint = " ", Info = " " }

function M.setup()
	-- Define a common set of configs that lsp handlers can use
	local handler_config = {
		float = {
			focusable = true,
			style = "minimal",
			border = "rounded",
		},
		diagnostic = {
			virtual_text = { spacing = 4, prefix = "●", severity = vim.diagnostic.severity.ERROR },
			underline = true,
			update_in_insert = false,
			severity_sort = true,
			float = {
				focusable = true,
				style = "minimal",
				border = "rounded",
				source = "always",
				header = "",
				prefix = "",
			},
		},
	}

	vim.diagnostic.config(handler_config.diagnostic)

	-- Set handlers config
	vim.lsp.handlers["textDocument/hover"] = vim.lsp.with(vim.lsp.handlers.hover, handler_config.float)
	vim.lsp.handlers["textDocument/signatureHelp"] = vim.lsp.with(vim.lsp.handlers.signature_help, handler_config.float)

	vim.lsp.handlers["workspace/diagnostic/refresh"] = function(_, _, ctx)
		local ns = vim.lsp.diagnostic.get_namespace(ctx.client_id)
		pcall(vim.diagnostic.reset, ns)
		return true
	end

	for type, icon in pairs(M.signs) do
		local hl = "DiagnosticSign" .. type
		vim.fn.sign_define(hl, { text = icon, texthl = hl, numhl = "" })
	end
end

return M
