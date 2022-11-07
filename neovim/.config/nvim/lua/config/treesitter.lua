local M = {}

function M.setup()
	local parser_config = require("nvim-treesitter.parsers").get_parser_configs()
	parser_config.cue = {
		install_info = {
			url = "https://github.com/eonpatapon/tree-sitter-cue", -- local path or git repo
			files = { "src/parser.c", "src/scanner.c" },
			branch = "main",
		},
		filetype = "cue", -- if filetype does not agrees with parser name
	}

	require("nvim-treesitter.configs").setup({
		ensure_installed = "all",

		sync_install = false,

		ignore_install = {
			"phpdoc",
		},

		highlight = {
			enable = true,
			additional_vim_regex_highlighting = { "markdown" },
		},

		textobjects = {
			select = {
				enable = true,
				lookahead = true,
				include_surrounding_whitespace = true,
				keymaps = {
					["af"] = "@function.outer",
					["if"] = "@function.inner",

					["ac"] = "@class.outer",
					["ic"] = "@class.inner",

					["ia"] = "@parameter.inner",
					["aa"] = "@parameter.outer",

					["il"] = "@loop.inner",
					["al"] = "@loop.inner",

					["ao"] = "@comment.outer",
					["io"] = "@comment.inner",
				},
			},
			swap = {
				enable = true,
				swap_next = {
					["<leader>cx"] = "@parameter.inner",
				},
				swap_previous = {
					["<leader>cX"] = "@parameter.inner",
				},
			},
			move = {
				enable = true,
				set_jumps = false,
				goto_next_start = {
					["]m"] = "@function.outer",
					["]]"] = "@class.outer",
				},
				goto_next_end = {
					["]M"] = "@function.outer",
					["]["] = "@class.outer",
				},
				goto_previous_start = {
					["[m"] = "@function.outer",
					["[["] = "@class.outer",
				},
				goto_previous_end = {
					["[M"] = "@function.outer",
					["[]"] = "@class.outer",
				},
			},
		},

		matchup = {
			enable = true,
		},

		-- context_commentstring = {},
		-- rainbow = {},
		-- autopairs = {},
		-- autotag = {},

		incremental_selection = {
			enable = true,
			keymaps = {
				init_selection = "gnn",
				node_incremental = "grn",
				scope_incremental = "grc",
				node_decremental = "grm",
			},
		},

		indent = {
			enable = true,
		},
	})
end

return M
