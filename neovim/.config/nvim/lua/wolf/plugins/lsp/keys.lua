local M = {}

function M.setup(client, buffer)
	local wk = require("which-key")
	local keymap = {
		buffer = buffer,
		["<leader>"] = {
			l = {
				name = "+lsp",
				{
					cond = client.Name == "tsserver",
					o = { "<cmd>TypescriptOrganizeImports<cr>", "Organize Imports" },
				},
				{
					cond = client.Name == "gopls",
				},
				a = {
					{ vim.lsp.buf.code_action, "Code Action" },
					{ "<cmd>lua vim.lsp.buf.code_action()<cr>", "Code Action", mode = "v" },
				},
				s = { "<cmd>AerialToggle!<cr>", "Symbols Outline" },
				d = { vim.diagnostic.open_float, "Line Diagnostics" },
				D = { "<cmd>Neogen<cr>", "Create docs" },
				i = { "<cmd>LspInfo<cr>", "Lsp Info" },
				r = { vim.lsp.buf.rename, "Rename" },
				R = { "<cmd>LspRestart " .. client.id .. "<cr>", "Restart" },
				w = {
					name = "+workspace",
					a = { "<cmd>lua vim.lsp.buf.add_workspace_folder()<cr>", "Add Folder to Workspace" },
					d = { "<cmd>lua vim.lsp.buf.remove_workspace_folder()<cr>", "Remove Folder from Workspace" },
					l = {
						"<cmd>lua print(vim.inspect(vim.lsp.buf.list_workspace_folders()))<cr>",
						"List Folders in Workspace",
					},
				},
			},
		},
		g = {
			name = "+goto",
			a = { vim.lsp.buf.code_action, "Code Action" },
			d = { require("telescope.builtin").lsp_definitions, "Definition" },
			r = { require("telescope.builtin").lsp_references, "References" },
			s = { require("telescope.builtin").lsp_implementations, "Implementations" },  -- Avoid using "i" to not overlap with "gi"
			t = { require("telescope.builtin").lsp_type_definitions, "Type Definitions" },
		},
		["<C-h>"] = { vim.lsp.buf.signature_help, "Signature Help", mode = { "i" } },
		["K"] = { vim.lsp.buf.hover, "Hover" },
		["[d"] = { vim.diagnostic.goto_prev, "Prev Diagnostic" },
		["]d"] = { vim.diagnostic.goto_next, "Next Diagnostic" },
		["[e"] = {
			"<cmd> lua vim.diagnostic.goto_prev({ severity = vim.diagnostic.severity.ERROR })<cr>",
			"Prev Diagnostic",
		},
		["]e"] = {
			"<cmd> lua vim.diagnostic.goto_next({ severity = vim.diagnostic.severity.ERROR })<cr>",
			"Next Diagnostic",
		},
	}

	wk.register(keymap)
end

return M
