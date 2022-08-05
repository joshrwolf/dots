local M = {}

function M.setup()
	local whichkey = require("which-key")

	local conf = {
		window = {
			border = "single",
			position = "bottom",
		},
		plugins = {
			marks = true,
			registers = true,
		},
	}

	whichkey.setup(conf)

	-- Register generic mappings
	whichkey.register({
		["\\"] = { "<cmd>Neotree toggle<cr>", "Toggle File Explorer" },
		["|"] = { "<cmd>Neotree buffers<cr>", "Toggle Buffer Explorer" },
		["<C-\\>"] = { "<cmd>ToggleTerm direction='float'<cr>", "Toggle Floating Terminal" },

		t = {
			name = "Tab",
			d = { "<cmd>:tabclose<cr>", "Close Tab" },
			h = { "<cmd>:tabfirst<cr>", "First Tab" },
			j = { "<cmd>:tabprev<cr>", "Prev Tab" },
			k = { "<cmd>:tabnext<cr>", "Next Tab" },
			l = { "<cmd>:tablast<cr>", "Last Tab" },
		},
	})

	-- Register leader based mappings
	whichkey.register({
		w = { "<cmd>:update<cr>", "Save" },
		q = { "<cmd>q<cr>", "Quit" },

		[","] = { "<cmd>lua require('telescope.builtin').find_files()<cr>", "Find Files" },
		["."] = { "<cmd>lua require('telescope.builtin').buffers()<cr>", "Buffers" },

		b = {
			name = "Buffers",
			c = { "<cmd>bd!<CR>", "Close current buffer" },
		},

		f = {
			name = "Find",
			f = { "<cmd>lua require('telescope.builtin').find_files()<cr>", "Files" },
			w = { "<cmd>lua require('telescope.builtin').live_grep()<cr>", "Live Grep" },
			b = { "<cmd>lua require('telescope.builtin').buffers()<cr>", "Bufers" },
			d = { "<cmd>lua require('telescope').extensions.file_browser.file_browser()<cr>", "File Browser" },
			a = {
				"<cmd>lua require('telescope').extensions.file_browser.file_browser( { path = '%:p:h' } )<cr>",
				"Active File Browser",
			},
			h = { "<cmd>lua require('telescope.builtin').help_tags()<cr>", "Help Tags" },
			t = { "<cmd>TodoTelescope<cr>", "Todos" },
			["."] = { "<cmd>lua require('telescope.builtin').resume()<cr>", "Resume" },
		},

		p = {
			name = "Packer",
			c = { "<cmd>PackerCompile<cr>", "Compile" },
			i = { "<cmd>PackerInstall<cr>", "Install" },
			s = { "<cmd>PackerSync<cr>", "Sync" },
			S = { "<cmd>PackerStatus<cr>", "Status" },
			u = { "<cmd>PackerUpdate<cr>", "Update" },
			p = { "<cmd>PackerProfile<cr>", "Profile" },
		},

		t = {
			name = "Terminal",
			f = { "<cmd>ToggleTerm direction=float<cr>", "New Float" },
			h = { "<cmd>ToggleTerm size=10 direction=horizontal<cr>", "New Horizontal" },
			v = { "<cmd>ToggleTerm size=80 direction=vertical<cr>", "New Vertical" },

			F = { "<cmd>ToggleTerm direction=float dir='%:p:h'<cr>", "New Float $CWD" },
			H = { "<cmd>ToggleTerm size=10 direction=horizontal dir='%:p:h'<cr>", "New Horizontal $CWD" },
			V = { "<cmd>ToggleTerm size=80 direction=vertical dir='%:p:h'<cr>", "New Vertical $CWD" },
		},

		j = {
			name = "Jump",
			a = { "<cmd>lua require('harpoon.mark').add_file()<cr>", "Add File" },
			j = { "<cmd>lua require('harpoon.ui').toggle_quick_menu()<cr>", "UI Menu" },
			o = { "<cmd>lua require('harpoon.cmd-ui').toggle_quick_menu()<cr>", "Command Menu" },
		},
		["1"] = { "<cmd>lua require('harpoon.ui').nav_file(1)<cr>", "Harpoon 1" },
		["2"] = { "<cmd>lua require('harpoon.ui').nav_file(2)<cr>", "Harpoon 2" },
		["3"] = { "<cmd>lua require('harpoon.ui').nav_file(3)<cr>", "Harpoon 3" },
		["9"] = { "<cmd>lua require('harpoon.term').gotoTerminal(1)<cr>", "Terminal 1" },
		["0"] = { "<cmd>lua require('harpoon.term').gotoTerminal(2)<cr>", "Terminal 2" },

		l = {
			name = "LSP",
			d = { "<cmd>Telescope diagnostics<CR>", "Diagnostics" },
			i = { "<cmd>LspInfo<CR>", "Lsp Info" },
			I = { "<cmd>Mason<CR>", "Mason Info" },
		},

		g = {
			name = "Git",
			g = { "<cmd>Neogit<cr>", "Neogit" },
			l = { "<cmd>Neogit log<cr>", "Git Log" },
			c = { "<cmd>lua require('config.telescope').delta_git_commits()<cr>", "Delta Commits" },
			t = { "<cmd>lua require('config.telescope').delta_git_status()<cr>", "Delta Status" },
			d = { "<cmd>DiffviewOpen<cr>", "Diff View" },
			D = { "<cmd>DiffviewOpen main<cr>", "Diff View Main" },
			f = { "<cmd>DiffviewFileHistory<cr>", "Diff View File History" },
			F = { "<cmd>DiffviewFileHistory %<cr>", "Diff View File History Current File" },
			b = { "<cmd>Gitsigns blame_line<cr>", "Blame Line" },
			B = { "<cmd>G blame<cr>", "Blame File" },
			["\\"] = { "<cmd>Neotree git_status<Cr>", "Git Reveal" },
		},

		x = {
			name = "Trouble",
			x = { "<cmd>Trouble<cr>", "Trouble" },
			w = { "<cmd>Trouble workspace_diagnostics<cr>", "Workspace Diagnostics" },
			d = { "<cmd>Trouble document_diagnostics<cr>", "Document Diagnostics" },
			l = { "<cmd>Trouble loclist<cr>", "Loc List" },
			q = { "<cmd>Trouble quickfix<cr>", "Quickfix" },
			R = { "<cmd>Trouble lsp_references<cr>", "LSP References" },
		},

		["<leader>"] = {
			name = "Woah",
			["s"] = { "<cmd>source ~/.config/nvim/after/plugin/luasnip.lua<CR>", "Reload Snippets" },
			["r"] = { ":source ~/.config/nvim/init.lua<cr>", "Reload Neovim Configs" },
			["w"] = { "<cmd>:wall<cr>", "Save All" },
			["q"] = { "<cmd>:quitall<cr>", "Quit All" },
		},
	}, { prefix = "<leader>", mode = "n", buffer = nil, silent = true, noremap = true, nowait = false })
end

return M
