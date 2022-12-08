local M = {}

function M.setup()
	require("mason").setup()

	local lspconfig = require("lspconfig")
	local lsp_cmp = require("cmp_nvim_lsp")

	local capabilities
	do
		local default_capabilities = vim.lsp.protocol.make_client_capabilities()
	end

	require("mason-lspconfig").setup({})

	require("mason-lspconfig").setup_handlers({
		function(server_name)
			lspconfig[server_name].setup({})
		end,
		["sumneko_lua"] = function() end,
	})
end

return M
