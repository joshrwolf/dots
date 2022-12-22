local M = {
	"hrsh7th/nvim-cmp",
	event = "InsertEnter",
	dependencies = {
		"hrsh7th/cmp-nvim-lsp",
		"hrsh7th/cmp-nvim-lsp-signature-help",
		"hrsh7th/cmp-buffer",
		"hrsh7th/cmp-cmdline",
		"dmitmel/cmp-cmdline-history",
		"hrsh7th/cmp-path",
		"lukas-reineke/cmp-rg",
		"L3MON4D3/LuaSnip",
		"saadparwaiz1/cmp_luasnip",
	},
}

function M.config()
	vim.o.completeopt = "menuone,noselect"

	local cmp = require("cmp")
	local luasnip = require("luasnip")

	cmp.setup({
		completion = {
			completeopt = "menu,menuone,noinsert",
		},
		snippet = {
			expand = function(args)
				luasnip.lsp_expand(args.body)
			end,
		},
		mapping = cmp.mapping.preset.insert({
			["<C-space>"] = cmp.mapping.complete(),
			["<CR>"] = cmp.mapping.confirm({
				behavior = cmp.ConfirmBehavior.Replace,
				select = true,
			}),
			["<C-u>"] = cmp.mapping.scroll_docs(-4),
			["<C-d>"] = cmp.mapping.scroll_docs(4),
			["<C-j>"] = cmp.mapping.select_next_item(),
			["<C-k>"] = cmp.mapping.select_prev_item(),
		}),
		sources = {
			{ name = "nvim_lsp" },
			{ name = "nvim_lsp_signature_help" },
			{ name = "buffer", keyword_length = 4 },
			{ name = "luasnip" },
			{ name = "rg", keyword_length = 5 },
		},
		formatting = {
			format = function(_, vim_item)
				if M.icons[vim_item.kind] then
					vim_item.kind = M.icons[vim_item.kind] .. vim_item.kind
				end
				return vim_item
			end,
		},
	})
end

M.icons = {
	Class = " ",
	Color = " ",
	Constant = " ",
	Constructor = " ",
	Enum = "了 ",
	EnumMember = " ",
	Field = " ",
	File = " ",
	Folder = " ",
	Function = " ",
	Interface = "ﰮ ",
	Keyword = " ",
	Method = "ƒ ",

	Property = " ",
	Snippet = "﬌ ",
	Struct = " ",
	Text = " ",
	Unit = " ",
	Value = " ",
	Variable = " ",
}

return M
