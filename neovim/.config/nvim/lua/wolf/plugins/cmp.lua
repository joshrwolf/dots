return {
	-- snippets
	{
		"L3MON4D3/LuaSnip",
		dependencies = {
			"rafamadriz/friendly-snippets",
			config = function()
				require("luasnip.loaders.from_vscode").lazy_load()
			end,
		},
		opts = {
			history = true,
			delete_check_events = "TextChanged",
		},
		keys = {
			{
				"<tab>",
				function()
					return require("luasnip").jumpable(1) and require("luasnip").jump(1) or "<tab>"
				end,
				expr = true,
				remap = true,
				silent = true,
				mode = "i",
			},
			{
				"<tab>",
				function()
					require("luasnip").jump(1)
				end,
				mode = "s",
			},
			{
				"<s-tab>",
				function()
					require("luasnip").jump(-1)
				end,
				mode = { "i", "s" },
			},
		},
	},

	-- completion
	{
		"hrsh7th/nvim-cmp",
		event = "InsertEnter",
		dependencies = {
			"hrsh7th/cmp-nvim-lsp",
			"hrsh7th/cmp-nvim-lsp-signature-help",
			"hrsh7th/cmp-buffer",
			{
				"petertriho/cmp-git",
				ft = "git",
				config = function()
					require("cmp_git").setup({})
				end,
			},
			"saadparwaiz1/cmp_luasnip",
		},
		config = function()
			local completeopt = "menu,menuone,noinsert"
			vim.o.completeopt = completeopt

			local cmp = require("cmp")
			cmp.setup({
				completion = {
					completeopt = completeopt,
				},
				snippet = {
					expand = function(args)
						require("luasnip").lsp_expand(args.body)
					end,
				},
				mapping = cmp.mapping.preset.insert({
					["<c-b>"] = cmp.mapping.scroll_docs(-4),
					["<c-f>"] = cmp.mapping.scroll_docs(4),
					["<c-space>"] = cmp.mapping.complete({}),
					["<c-e>"] = cmp.mapping.abort(),
					["<cr>"] = cmp.mapping.confirm({ select = true, behavior = cmp.ConfirmBehavior.Replace }),
					["<tab>"] = cmp.mapping.confirm({ select = true, behavior = cmp.ConfirmBehavior.Replace }),
					["<c-j>"] = cmp.mapping.select_next_item(),
					["<c-k>"] = cmp.mapping.select_prev_item(),
				}),
				sources = cmp.config.sources({
					{ name = "nvim_lsp" },
					{ name = "nvim_lsp_signature_help" },
					{ name = "buffer", keyword_length = 3, max_item_count = 3 },
					{ name = "luasnip", keyword_length = 2, max_item_count = 3 },
					{ name = "path" },
				}),
				formatting = {
					fields = { "abbr", "kind", "menu" },
					format = function(entry, vim_item)
						vim_item.kind = (require("wolf.config.settings").icons.kinds[vim_item.kind] or "")
							.. vim_item.kind
						vim_item.menu = ({
							nvim_lsp = "[LSP]",
							buffer = "[Buf]",
							luasnip = "[SNIP]",
							nvim_lua = "[LUA]",
							git = "[GIT]",
							path = "[PATH]",
							rg = "[RG]",
						})[entry.source.name]
						return vim_item
					end,
				},
				sorting = {
					-- TODO: Refine this over time
					-- refs:
					--  - https://github.com/Bekaboo/nvim/blob/master/lua/modules/completion/configs.lua#L127
					comparators = {
						cmp.config.compare.kind,
						cmp.config.compare.locality,
						cmp.config.compare.recently_used,
						cmp.config.compare.exact,
						cmp.config.compare.score,
					},
				},
			})

			cmp.setup.filetype("gitcommit", {
				sources = cmp.config.sources({
					{ name = "git" },
					{ name = "buffer" },
				}),
			})
		end,
	},

	-- autopairs
	{
		"echasnovski/mini.pairs",
		event = "InsertEnter",
		config = function(_, opts)
			require("mini.pairs").setup(opts)
		end,
	},
}
