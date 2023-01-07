local M = {
	"neovim/nvim-lspconfig",
	name = "lsp",
	event = "BufReadPre",

	dependencies = {
		"williamboman/mason.nvim",
		"williamboman/mason-lspconfig.nvim",

		"j-hui/fidget.nvim",
		"folke/neodev.nvim",

		"jose-elias-alvarez/null-ls.nvim",
		"lvimuser/lsp-inlayhints.nvim",

		"b0o/SchemaStore.nvim",
		"SmiteshP/nvim-navic",
		{
			"stevearc/aerial.nvim",
			dependencies = { "onsails/lspkind.nvim" },
		},
		{
			"kosayoda/nvim-lightbulb",
			dependencies = { "antoinemadec/FixCursorHold.nvim" },
		},
	},
}

local on_attach = function(client, bufnr)
	require("wolf.plugins.lsp.diagnostics").setup()
	require("wolf.plugins.lsp.formatting").setup(client, bufnr)
	require("wolf.plugins.lsp.keys").setup(client, bufnr)

	if client.server_capabilities.completionProvider then
		vim.bo[bufnr].omnifunc = "v:lua.vim.lsp.omnifunc"
		vim.bo[bufnr].completefunc = "v:lua.vim.lsp.omnifunc"
	end

	if client.server_capabilities.definitionProvider then
		vim.bo[bufnr].tagfunc = "v:lua.vim.lsp.tagfunc"
	end

	if client.server_capabilities["textDocument/codeLens"] then
		vim.api.nvim_create_autocmd({ "CursorHold", "CompleteDone" }, {
			buffer = bufnr,
			desc = "LSP: Refresh Code Lens",
			callback = function()
				require("vim.lsp.codelens").refresh()
			end,
		})
	end

	-- setup nvim-navic if documentSymbolProvider exists
	if client.server_capabilities.documentSymbolProvider then
		require("nvim-navic").attach(client, bufnr)
	end

	-- setup inlay hints
	require("lsp-inlayhints").on_attach(client, bufnr)

	require("nvim-lightbulb").setup({
		sign = { enabled = true }, -- "text" doesn't work here :(
		float = { enabled = false, text = "" },
		status_text = { enable = false },
		virtual_text = { enable = false },
		autocmd = { enabled = true },
	})

	require("aerial").setup({
		backends = { "lsp", "treesitter" },
		filter_kind = false,
		highlight_on_hover = true,
		show_guides = true,
		layout = {
			max_width = { 60, 0.3 },
			default_direction = "prefer_left",
		},
		guides = {
			mid_item = "├ ",
			last_item = "└ ",
			nested_top = "│ ",
			whitespace = "  ",
		},
	})
end

function M.config()
	require("neodev").setup()
	require("mason").setup()

	local mason_lspconfig = require("mason-lspconfig")

	local capabilities = vim.lsp.protocol.make_client_capabilities()
	capabilities = require("cmp_nvim_lsp").default_capabilities(capabilities)
	capabilities.textDocument.foldingRange = {
		dynamicRegistration = false,
		lineFoldingOnly = true,
	}

	-- default_opts are deep merged as defaults with all servers
	local default_opts = {
		on_attach = on_attach,
		capabilities = capabilities,
		flags = {
			debounce_text_changes = 150,
		},
	}

	-- servers is a table passed directly into lspconfig[server].setup()
	-- for example, setting up gopls looks like: lspconfig["gopls"].setup(servers["gopls"])
	local servers = {
		jsonls = {
			settings = {
				json = {
					schemas = require("schemastore").json.schemas(),
					validate = { enable = true },
				},
			},
		},
		rust_analyzer = {},
		gopls = {
			settings = {
				gopls = {
					["ui.documentation.linksInHover"] = false,
					["ui.diagnostics.staticcheck"] = false,
					codelenses = {
						generate = true,
					},
					semanticTokens = true,
					analyses = {
						assign = true,
						unreachable = true,
						unusedparams = true,
						bools = true,
						nilness = true,
						nilfunc = true,
						shadow = true,
						unusedwrite = true,
						unusedvariable = true,
					},
				},
			},
		},
		sumneko_lua = {
			settings = {
				Lua = {
					workspace = { checkThirdParty = false },
					telemetry = { enable = false },
					format = { enable = false },
					diagnostics = { globals = { "vim", "require" } },
					completion = {
						callSnippet = "Replace",
					},
				},
			},
		},
		yamlls = {
			settings = {
				yaml = {
					schemaStore = {
						enable = true,
						url = "https://www.schemastore.org/api/json/catalog.json",
					},
					schemas = {
						["http://json.schemastore.org/github-workflow"] = ".github/workflows/*",
						["http://json.schemastore.org/github-action"] = ".github/action.{yml,yaml}",
						["http://json.schemastore.org/chart"] = "Chart.{yml,yaml}",
						["http://json.schemastore.org/kustomization"] = "kustomization.{yml,yaml}",
					},
					format = { enabled = false },
					validate = false,
					completion = true,
					hover = true,
				},
			},
		},
	}

	-- setup_handlers runs for each installed lsp_server
	mason_lspconfig.setup_handlers({
		function(server_name)
			-- opts represents default_opts merged with server[server_name]
			local opts = vim.tbl_deep_extend("force", {}, default_opts, servers[server_name] or {})
			require("lspconfig")[server_name].setup(opts)
		end,
	})

	require("fidget").setup({
		text = {
			spinner = "dots",
			done = "",
		},
	})
end

return M
