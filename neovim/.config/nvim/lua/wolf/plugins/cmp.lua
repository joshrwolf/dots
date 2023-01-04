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
		"petertriho/cmp-git",
		"onsails/lspkind.nvim",
	},
}

function M.config()
	vim.o.completeopt = "menuone,noselect"

	local cmp = require("cmp")
	local luasnip = require("luasnip")

	require("cmp_git").setup({
		filetypes = { "*" },
	})

	cmp.setup({
		completion = {
			completeopt = "menu,menuone,noselect",
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
				select = false,
			}),
			["<Tab>"] = cmp.mapping(function(fallback)
				if cmp.visible() then
					local entry = cmp.get_selected_entry()
					if not entry then
						cmp.select_next_item({ behavior = cmp.SelectBehavior.Select })
						cmp.confirm()
					else
						cmp.confirm()
					end
				else
					fallback()
				end
			end, { "i", "s", "c" }),
			["<C-u>"] = cmp.mapping.scroll_docs(-4),
			["<C-d>"] = cmp.mapping.scroll_docs(4),
			["<C-j>"] = cmp.mapping.select_next_item(),
			["<C-k>"] = cmp.mapping.select_prev_item(),
		}),
		sources = {
			{ name = "nvim_lsp" },
			{ name = "nvim_lsp_signature_help" },
			{ name = "luasnip" },
			{ name = "path" },
			{ name = "buffer", keyword_length = 4 },
			{ name = "rg", keyword_length = 5, option = { additional_arguments = "--max-depth 8" } },
			{ name = "git" }, -- only triggers on @,#,!,:
		},
		formatting = {
			fields = { "kind", "abbr", "menu" },
			format = require("lspkind").cmp_format({
				mode = "symbol",
				menu = {
					nvim_lsp = "[LSP]",
					buffer = "[BUF]",
					luasnip = "[SNIP]",
					nvim_lua = "[LUA]",
					git = "[GIT]",
					path = "[PATH]",
					rg = "[RG]",
				},
			}),
		},
	})

	cmp.setup.filetype("gitcommit", {
		sources = cmp.config.sources({
			{ name = "git" }, -- only triggers on @,#,!,:
			{ name = "buffer" },
		}),
	})
end

return M
