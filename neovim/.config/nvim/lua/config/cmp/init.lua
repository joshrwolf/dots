local M = {}

vim.opt.completeopt = "menu,menuone,noinsert"

local types = require("cmp.types")
-- local lspkind = require("lspkind")
local icons = require("config.icons").icons

function M.setup()
	local luasnip = require("luasnip")
	-- local neogen = require "neogen"
	local cmp = require("cmp")

	cmp.setup({
		completion = {
			completeopt = "menuone,noinsert",
			autocomplete = { types.cmp.TriggerEvent.TextChanged },
			keyword_length = 1,
		},
		snippet = {
			expand = function(args)
				require("luasnip").lsp_expand(args.body)
			end,
		},
		formatting = {
			-- fields = { "kind", "abbr", "menu" },
			format = function(entry, vim_item)
				vim_item.kind = string.format("%s %s", icons.kind[vim_item.kind], vim_item.kind)
				vim_item.menu = ({
					nvim_lsp = "[LSP]",
					luasnip = "[SNP]",
					buffer = "[BUF]",
					path = "[PTH]",
				})[entry.source.name]

				return vim_item
			end,
		},
		mapping = {
			["<C-e>"] = cmp.mapping({ i = cmp.mapping.close(), c = cmp.mapping.close() }),
			["<C-k>"] = cmp.mapping(cmp.mapping.select_prev_item(), { "i", "c" }),
			["<C-p>"] = cmp.mapping(cmp.mapping.select_prev_item(), { "i", "c" }),
			["<C-j>"] = cmp.mapping(cmp.mapping.select_next_item(), { "i", "c" }),
			["<C-n>"] = cmp.mapping(cmp.mapping.select_next_item(), { "i", "c" }),
			["<C-u>"] = cmp.mapping.scroll_docs(-4),
			["<C-d>"] = cmp.mapping.scroll_docs(4),
			["<C-Space>"] = cmp.mapping(cmp.mapping.complete(), { "i", "s", "c" }),
			["<CR>"] = cmp.mapping.confirm({ select = true, behavior = cmp.ConfirmBehavior.Replace }),
			["<Tab>"] = cmp.mapping(function(fallback)
				if cmp.visible() then
					local entry = cmp.get_selected_entry()
					if not entry then
						cmp.select_next_item({ behavior = cmp.SelectBehavior.Select })
					else
						cmp.confirm({ select = false, behavior = cmp.ConfirmBehavior.Replace })
					end
				else
					fallback()
				end
			end, { "i", "s", "c" }),
		},
		sources = cmp.config.sources({
			{ name = "nvim_lsp" },
			{ name = "nvim_lsp_signature_help" },
			{ name = "nvim_lua" },
			{ name = "luasnip" },
			{ name = "path" },
		}, {
			{ name = "buffer" },
		}),
		window = {
			completion = {
				-- winhighlight = "Normal:Pmenu,FloatBorder:Pmenu",
				-- col_offset = -3,
				-- side_padding = 0,
			},
			-- completion = vim.tbl_deep_extend("force", cmp.config.window.bordered(), { col_offset = -1 }),
			documentation = cmp.config.window.bordered(),
			-- documentation = {
			--   -- border = { "╭", "─", "╮", "│", "╯", "─", "╰", "│" },
			--   -- winhighlight = "NormalFloat:NormalFloat,FloatBorder:TelescopeBorder",
			-- },
		},
		confirm_opts = {
			behavior = cmp.ConfirmBehavior.Replace,
			select = false,
		},
		experimental = { ghost_text = true },
		sorting = {
			comparators = {
				function(...)
					local cmp_buffer = require("cmp_buffer")
					cmp_buffer:compare_locality(...)
					-- require("cmp_buffer"):compare_locality(...)
				end,
				cmp.config.compare.exact,
				cmp.config.compare.locality,
				cmp.config.compare.recently_used,
				cmp.config.compare.score,
				cmp.config.compare.offset,
				cmp.config.compare.sort_text,
				cmp.config.compare.order,
			},
		},
	})

	-- special ft completions
	cmp.setup.filetype({ "markdown", "asciidoc", "text" }, {
		sources = {
			{ name = "path" },
		},
	})

	-- Use buffer source for `/`
	cmp.setup.cmdline("/", {
		mapping = cmp.mapping.preset.cmdline(),
		sources = {
			{ name = "buffer" },
		},
	})

	cmp.setup.cmdline(":", {
		-- mapping = cmp.mapping.preset.cmdline(),
		sources = cmp.config.sources({
			-- { name = "path" },
			{ name = "cmdline" },
		}),
	})

	require("cmp_git").setup({
		filetypes = { "gitcommit", "NeogitCommitMessage", "markdown", "octo" },
	})

	-- Auto pairs
	local cmp_autopairs = require("nvim-autopairs.completion.cmp")
	cmp.event:on("confirm_done", cmp_autopairs.on_confirm_done({ map_char = { tex = "" } }))
end

return M
