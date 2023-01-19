local M = {}

function M.on_attach(client, buffer)
	local self = M.new(client, buffer)

	self:map("gd", "Telescope lsp_definitions", { desc = "Goto Definition" })
	self:map("gr", "Telescope lsp_references", { desc = "References" })
	self:map("gD", "Telescope lsp_declarations", { desc = "Goto Declarations" })
	self:map("gI", "Telescope lsp_implementations", { desc = "Goto Implementations" })
	self:map("gt", "Telescope lsp_type_definitions", { desc = "Goto Type Definition" })
	self:map("K", vim.lsp.buf.hover, { desc = "Hover" })
	self:map("gK", vim.lsp.buf.signature_help, { desc = "Signature Help", has = "signatureHelp" })
	self:map("<c-h>", vim.lsp.buf.signature_help, { desc = "Signature Help", mode = { "i" }, has = "signatureHelp" })

	self:map("[d", M.diagnostic_goto(false), { desc = "Prev Diagnostic" })
	self:map("]d", M.diagnostic_goto(true), { desc = "Next Diagnostic" })
	self:map("[e", M.diagnostic_goto(false, "ERROR"), { desc = "Prev Error" })
	self:map("]e", M.diagnostic_goto(true, "ERROR"), { desc = "Next Error" })

	self:map("<leader>ca", vim.lsp.buf.code_action, { desc = "Code Action", mode = { "n", "v" }, has = "codeAction" })
	self:map("<leader>cr", vim.lsp.buf.rename, { desc = "Rename", has = "rename" })
	self:map("<leader>cR", function() end, { desc = "Restart Lsp" })
	self:map("<leader>cd", vim.diagnostic.open_float, { desc = "Line Diagnostic" })
	self:map("<leader>ci", "LspInfo", { desc = "Lsp Info" })

	local format = require("wolf.plugins.lsp.format").format
	self:map("<leader>cf", format, { desc = "Format Document", has = "documentFormatting" })
	self:map("<leader>cf", format, { desc = "Format Range", mode = "v", has = "documentRangeFormatting" })
	self:map("<leader>cf", format, { desc = "Format Range", mode = "v", has = "documentRangeFormatting" })
	self:map("<leader>cF", function()
		require("wolf.plugins.lsp.format").toggle()
	end, { desc = "Toggle Formatting", has = "documentRangeFormatting" })
end

function M.new(client, buffer)
	return setmetatable({ client = client, buffer = buffer }, { __index = M })
end

-- use to check whether lsp has capabilities
function M:has(cap)
	return self.client.server_capabilities[cap .. "Provider"]
end

function M:map(lhs, rhs, opts)
	opts = opts or {}
	if opts.has and not self:has(opts.has) then
		return
	end
	vim.keymap.set(
		opts.mode or "n",
		lhs,
		type(rhs) == "string" and ("<cmd>%s<cr>"):format(rhs) or rhs,
		---@diagnostic disable-next-line: no-unknown
		{ silent = true, buffer = self.buffer, expr = opts.expr, desc = opts.desc }
	)
end

function M.diagnostic_goto(next, severity)
	local go = next and vim.diagnostic.goto_next or vim.diagnostic.goto_prev
	severity = severity and vim.diagnostic.severity[severity] or nil
	return function()
		go({ severity = severity })
	end
end

return M
