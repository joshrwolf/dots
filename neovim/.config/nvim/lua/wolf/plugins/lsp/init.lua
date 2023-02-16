return {
	-- lspconfig
	{
		"neovim/nvim-lspconfig",
		event = "BufReadPre",
		dependencies = {
			{ "folke/neoconf.nvim", cmd = "Neoconf", config = true },
			{
				"folke/neodev.nvim",
				opts = {
					library = {
						plugins = { "neotest" },
						types = true,
					},
				},
			},
			"williamboman/mason.nvim",
			"williamboman/mason-lspconfig.nvim",
			"hrsh7th/cmp-nvim-lsp", -- ensure lazy loads properly

			"b0o/SchemaStore.nvim",

			"simrat39/rust-tools.nvim",

			{
				"j-hui/fidget.nvim",
				opts = { text = { spinner = "dots", done = "" } },
			},
		},
		opts = {
			diagnostics = {
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
			},
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
							env = {
								GOFLAGS = "-tags=e2e",
							},
						},
					},
				},
				terraformls = {},
				tsserver = {},
				bufls = {},
				lua_ls = {
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
			setup = {
				rust_analyzer = function(_, opts)
					local rust_tools_opts = vim.tbl_deep_extend("force", opts, {
						settings = {
							["rust-analyzer"] = {
								cargo = {
									allFeatures = true,
									loadOutDirsFromCheck = true,
									runBuildScripts = true,
								},
								-- Add clippy lints for Rust.
								checkOnSave = {
									allFeatures = true,
									command = "clippy",
									extraArgs = { "--no-deps" },
								},
								procMacro = {
									enable = true,
									ignored = {
										["async-trait"] = { "async_trait" },
										["napi-derive"] = { "napi" },
										["async-recursion"] = { "async_recursion" },
									},
								},
							},
						},
					})
					require("rust-tools").setup(rust_tools_opts)
					return true
				end,
			},
		},
		config = function(plugin, opts)
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
			vim.diagnostic.config(opts.diagnostics)

			-- configure popup handler
			local popup_config = {
				focusable = true,
				style = "minimal",
				border = "rounded",
			}
			vim.lsp.handlers["textDocument/hover"] = vim.lsp.with(vim.lsp.handlers.hover, popup_config)
			vim.lsp.handlers["textDocument/signatureHelp"] = vim.lsp.with(vim.lsp.handlers.signature_help, popup_config)

			-- setup servers
			local servers = opts.servers
			local capabilities =
				require("cmp_nvim_lsp").default_capabilities(vim.lsp.protocol.make_client_capabilities())

			local function setup(server)
				local server_opts = vim.tbl_deep_extend("force", {
					capabilities = vim.deepcopy(capabilities),
				}, servers[server] or {})

				if opts.setup[server] then
					if opts.setup[server](server, server_opts) then
						return
					end
				elseif opts.setup["*"] then
					if opts.setup["*"](server, server_opts) then
						return
					end
				end
				require("lspconfig")[server].setup(server_opts)
			end

			-- temp fix for lspconfig rename
			-- https://github.com/neovim/nvim-lspconfig/pull/2439
			local mappings = require("mason-lspconfig.mappings.server")
			if not mappings.lspconfig_to_package.lua_ls then
				mappings.lspconfig_to_package.lua_ls = "lua-language-server"
				mappings.package_to_lspconfig["lua-language-server"] = "lua_ls"
			end

			local mlsp = require("mason-lspconfig")
			local available = mlsp.get_available_servers()

			local ensure_installed = {} ---@type string[]
			for server, server_opts in pairs(servers) do
				if server_opts then
					server_opts = server_opts == true and {} or server_opts
					-- run manual setup if mason=false or if this is a server that cannot be installed with mason-lspconfig
					if server_opts.mason == false or not vim.tbl_contains(available, server) then
						setup(server)
					else
						ensure_installed[#ensure_installed + 1] = server
					end
				end
			end

			require("mason-lspconfig").setup({ ensure_installed = ensure_installed })
			require("mason-lspconfig").setup_handlers({ setup })
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
					nls.builtins.formatting.cue_fmt,
					-- nls.builtins.formatting.cueimports,  -- doesn't support import aliases :(

					nls.builtins.diagnostics.actionlint,
					nls.builtins.diagnostics.buf,
					-- nls.builtins.diagnostics.cue_fmt,
					-- nls.builtins.diagnostics.vale,

					-- nls.builtins.code_actions.shellcheck,
					-- nls.builtins.code_actions.gitrebase,
					nls.builtins.code_actions.gomodifytags,
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
			"delve",
			"gotests",
			"iferr",
			"gomodifytags",

			"rustfmt",
			"codelldb",

			"actionlint",

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
