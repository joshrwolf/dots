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
	vim.o.completeopt = "menu,menuone,noinsert"

	local cmp = require("cmp")
	local luasnip = require("luasnip")

	require("cmp_git").setup({
		filetypes = { "*" },
	})

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
			["<Tab>"] = cmp.mapping(function(fallback)
				if vim.fn.mode() == "i" then
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
				elseif vim.fn.mode() == "c" then
					if cmp.visible() then
						cmp.select_next_item()
					else
						cmp.complete()
					end
				end
			end, { "i", "c" }),
			["<C-u>"] = cmp.mapping.scroll_docs(-4),
			["<C-d>"] = cmp.mapping.scroll_docs(4),
			["<C-j>"] = cmp.mapping.select_next_item(),
			["<C-k>"] = cmp.mapping.select_prev_item(),
		}),
		sources = {
			{ name = "nvim_lsp", max_item_count = 15 },
			{ name = "nvim_lsp_signature_help" },
			{ name = "luasnip", keyword_length = 2, max_item_count = 2 },
			{ name = "path" }, -- only triggers on .
			{ name = "buffer", keyword_length = 4, max_item_count = 2 },
			-- { name = "rg", max_item_count = 2, keyword_length = 5, option = { additional_arguments = "--max-depth 8" } },
			-- { name = "git", entry_filter = function (_, _)
			--      -- Only use this source in comments
			--      local context = require("cmp.config.context")
			--      return context.in_treesitter_capture("comment") == true or context.in_syntax_group("Comment")
			-- end }, -- only triggers on @,#,!,:
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
