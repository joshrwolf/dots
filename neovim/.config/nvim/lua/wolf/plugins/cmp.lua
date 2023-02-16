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
				"<c-n>",
				function()
					if require("luasnip").jumpable(1) then
						require("luasnip").jump(1)
					end
				end,
				mode = { "i", "s" },
			},
			{
				"<c-p>",
				function()
					if require("luasnip").jumpable(-1) then
						require("luasnip").jump(-1)
					end
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
					["<c-u>"] = cmp.mapping.scroll_docs(-4),
					["<c-d>"] = cmp.mapping.scroll_docs(4),
					["<c-space>"] = cmp.mapping.complete({}),
					["<c-e>"] = cmp.mapping.abort(),
					["<cr>"] = cmp.mapping.confirm({ select = true, behavior = cmp.ConfirmBehavior.Replace }),
					["<tab>"] = cmp.mapping.confirm({ select = true, behavior = cmp.ConfirmBehavior.Replace }),
					["<c-j>"] = cmp.mapping.select_next_item(),
					["<c-k>"] = cmp.mapping.select_prev_item(),
				}),
				sources = cmp.config.sources({
					{ name = "nvim_lsp", max_item_count = 75 }, -- some lsp's return tons of crap (typescript)
					{ name = "nvim_lsp_signature_help", max_item_count = 25 },
					{ name = "luasnip", keyword_length = 2, max_item_count = 3 },
					{ name = "path" }, -- triggers on "."
				}, {
					{ name = "buffer", keyword_length = 3 },
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
				-- sorting = {
				-- 	-- TODO: Refine this over time
				-- 	-- refs:
				-- 	--  - https://github.com/Bekaboo/nvim/blob/master/lua/modules/completion/configs.lua#L127
				-- 	comparators = {
				-- 		cmp.config.compare.kind,
				-- 		cmp.config.compare.locality,
				-- 		cmp.config.compare.recently_used,
				-- 		cmp.config.compare.exact,
				-- 		cmp.config.compare.score,
				-- 	},
				-- },
				matching = {
					disallow_partial_fuzzy_matching = false,
				},
			})

			cmp.setup.filetype("gitcommit", {
				sources = cmp.config.sources({
					{ name = "git" },
					{ name = "buffer" },
				}),
			})

			vim.cmd([[highlight! CmpItemAbbrMatch gui=bold]])
			vim.cmd([[highlight! CmpItemAbbrMatchFuzzy gui=bold]])
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
