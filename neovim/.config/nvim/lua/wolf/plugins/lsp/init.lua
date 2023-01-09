return {
	-- lspconfig
	{
		"neovim/nvim-lspconfig",
		event = "BufReadPre",
		dependencies = {
			{ "folke/neoconf.nvim", cmd = "Neoconf", config = true },
			{ "folke/neodev.nvim", config = true },
			"williamboman/mason.nvim",
			"williamboman/mason-lspconfig.nvim",
			"hrsh7th/cmp-nvim-lsp", -- ensure lazy loads properly

			"b0o/SchemaStore.nvim",

			{
				"j-hui/fidget.nvim",
				opts = { text = { spinner = "dots", done = "" } },
			},
		},
		config = function(plugin)
			-- setup formatting and keymaps
			vim.api.nvim_create_autocmd("LspAttach", {
				callback = function(args)
					local buf = args.buf
					local client = vim.lsp.get_client_by_id(args.data.client_id)
					require("wolf.plugins.lsp.format").on_attach(client, buf)
					require("wolf.plugins.lsp.keymaps").on_attach(client, buf)

					if client.server_capabilities.documentSymbolProvider then
						require("nvim-navic").attach(client, buf)
					end
				end,
			})

			-- setup diagnostics
			for name, icon in pairs(require("wolf.config.settings").icons.diagnostics) do
				name = "DiagnosticSign" .. name
				vim.fn.sign_define(name, { text = icon, texthl = name, numhl = "" })
			end
			vim.diagnostic.config({
				underline = true,
				update_in_insert = false,
				virtual_text = { spacing = 4, prefix = "●", severity = vim.diagnostic.severity.ERROR },
				severity_sort = true,
				float = {
					focusable = true,
					style = "minimal",
					border = "rounded",
					source = "always",
					header = "",
					prefix = "",
				},
			})

			-- configure popup handler
			local popup_config = {
				focusable = true,
				style = "minimal",
				border = "rounded",
			}
			vim.lsp.handlers["textDocument/hover"] = vim.lsp.with(vim.lsp.handlers.hover, popup_config)
			vim.lsp.handlers["textDocument/signatureHelp"] = vim.lsp.with(vim.lsp.handlers.signature_help, popup_config)

			-- setup servers
			local servers = plugin.servers or {}
			local capabilities =
				require("cmp_nvim_lsp").default_capabilities(vim.lsp.protocol.make_client_capabilities())

			require("mason-lspconfig").setup({ ensure_installed = vim.tbl_keys(servers) })
			require("mason-lspconfig").setup_handlers({
				function(server)
					local opts = servers[server] or {}
					opts.capabilities = capabilities
					if not plugin.setup_server(server, opts) then
						require("lspconfig")[server].setup(opts)
					end
				end,
			})
		end,
		---@type lspconfig.options
		servers = {
			bashls = {},
			jsonls = {
				-- lazy-load the json schemastore only when needed
				on_new_config = function(new_config, new_root_dir)
					new_config.settings.json.schemas = new_config.settings.json.schemas or {}
					vim.list_extend(new_config.settings.json.schemas, require("schemastore").json.schemas())
				end,
				settings = {
					json = {
						format = { enable = true },
					},
					validate = { enable = true },
				},
			},
			yamlls = {},
			gopls = {
				settings = {
					gopls = {
						["ui.documentation.linksInHover"] = false,
						["ui.diagnostics.staticcheck"] = true,
						semanticTokens = false, -- turn on when 0.9 is stable
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
			rust_analyzer = {},
			terraformls = {},
			sumneko_lua = {
				settings = {
					Lua = {
						workspace = {
							checkThirdParty = false,
						},
						completion = {
							callSnippet = "Replace",
						},
					},
				},
			},
		},
		---@param server string lsp server name
		---@param opts _.lspconfig.options any options set for the server
		setup_server = function(server, opts)
			-- return true if this server should skip lsp setup
			return false
		end,
	},

	-- null-ls
	{
		"jose-elias-alvarez/null-ls.nvim",
		event = "BufReadPre",
		dependencies = { "williamboman/mason.nvim" },
		config = function()
			local nls = require("null-ls")
			nls.setup({
				sources = {
					nls.builtins.formatting.stylua,
					nls.builtins.formatting.terraform_fmt,
					nls.builtins.formatting.goimports,
					nls.builtins.formatting.gofumpt,
					nls.builtins.formatting.rustfmt,

					nls.builtins.diagnostics.actionlint,
					-- nls.builtins.diagnostics.buf,
					-- nls.builtins.diagnostics.vale,

					-- nls.builtins.code_actions.shellcheck,
					-- nls.builtins.code_actions.gitrebase,
				},
				-- TODO: Try this out?
				-- root_dir = require("null-ls.utils").root_pattern(".null-ls-root", ".neoconf.json", ".git"),
			})
		end,
	},

	-- mason
	{
		"williamboman/mason.nvim",
		cmd = "Mason",
		keys = {
			{ "<leader>pm", "<cmd>Mason<cr>", desc = "Mason" },
		},
		config = function(_self, opts)
			require("mason").setup()
			for _, tool in ipairs(_self.ensure_installed) do
				local p = require("mason-registry").get_package(tool)
				if not p:is_installed() then
					p:install()
				end
			end
		end,
		ensure_installed = {
			"stylua",
			-- "terraform_fmt",   -- this just runs "terraform fmt"
			"goimports",
			"gofumpt",
			"rustfmt",

			"actionlint",
			"shellcheck",
			"buf",

			"shellcheck",
		},
	},

	{
		"SmiteshP/nvim-navic",
		init = function()
			vim.g.navic_silence = true
			vim.api.nvim_create_autocmd("LspAttach", {
				callback = function(args)
					local buf = args.buf
					local client = vim.lsp.get_client_by_id(args.data.client_id)
					if client.server_capabilities.documentSymbolProvider then
						require("nvim-navic").attach(client, buf)
					end
				end,
			})
		end,
		opts = {
			highlight = true,
			depth_limit = 5,
		},
	},

	{
		-- better folding
		"kevinhwang91/nvim-ufo",
		event = "VeryLazy",
		enabled = false,
		dependencies = {
			"kevinhwang91/promise-async",
		},
		config = function()
			require("ufo").setup({
				close_fold_kinds = { "comment" },
			})

			vim.opt.foldcolumn = "1"
			vim.opt.foldlevel = 99
			vim.opt.foldlevelstart = -1
			vim.opt.foldenable = true
		end,
	},
}
