local M = {
	"L3MON4D3/LuaSnip",

	dependencies = {
		"rafamadriz/friendly-snippets",
		config = function()
			require("luasnip.loaders.from_vscode").lazy_load()
		end,
	},
}

function M.config()
	local luasnip = require("luasnip")

	luasnip.config.setup({
		history = true,
		enable_autosnippets = true,
	})

	-- TODO: Replace these mappings with something else
	vim.cmd([[
" press <Tab> to expand or jump in a snippet. These can also be mapped separately
" via <Plug>luasnip-expand-snippet and <Plug>luasnip-jump-next.
imap <silent><expr> <C-n> luasnip#expand_or_jumpable() ? '<Plug>luasnip-expand-or-jump' : '<Tab>' 
" -1 for jumping backwards.
inoremap <silent> <C-p> <cmd>lua require'luasnip'.jump(-1)<Cr>
snoremap <silent> <C-n> <cmd>lua require('luasnip').jump(1)<Cr>
snoremap <silent> <C-p> <cmd>lua require('luasnip').jump(-1)<Cr>
]])
end

return M
