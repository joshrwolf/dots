local U = {}

local fmt_group = vim.api.nvim_create_augroup("LspFormatting", { clear = true })

---Common format-on-save for lsp servers that implements formatting
---@param client table
---@param bufnr integer
function U.format_on_save(client, bufnr)
	if client.supports_method("textDocument/formatting") then
		vim.api.nvim_create_autocmd("BufWritePre", {
			group = fmt_group,
			buffer = bufnr,
			callback = function()
				vim.lsp.buf.format({
					timeout_ms = 2000,
					buffer = bufnr,
				})
			end,
		})
	end
end

return U
