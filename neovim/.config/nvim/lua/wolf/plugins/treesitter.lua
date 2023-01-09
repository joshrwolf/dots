function setup_cue()
	local parser_config = require("nvim-treesitter.parsers").get_parser_configs()
	parser_config.cue = {
		install_info = {
			url = "https://github.com/eonpatapon/tree-sitter-cue", -- local path or git repo
			files = { "src/parser.c", "src/scanner.c" },
			branch = "main",
		},
		filetype = "cue",
	}
end

return {
	{
		"nvim-treesitter/nvim-treesitter",
		build = ":TSUpdate",
		event = "BufReadPost",
		dependencies = {
			{ "nvim-treesitter/playground", cmd = "TSPlaygroundToggle" },
			"nvim-treesitter/nvim-treesitter-textobjects",
			"RRethy/nvim-treesitter-textsubjects",
		},
		opts = {
			sync_install = false, -- allows async downloading
			ensure_installed = {
				"bash",
				"diff",
				"gitignore",
				"go",
				"gomod",
				"gowork",
				"graphql",
				"hcl",
				"help",
				"html",
				"http",
				"javascript",
				"json",
				"jsonc",
				"json5",
				"lua",
				"markdown",
				"markdown_inline",
				"nix",
				"python",
				"query",
				"regex",
				"rust",
				"scss",
				"sql",
				"toml",
				"terraform",
				"tsx",
				"typescript",
				"vim",
				"yaml",

				-- customs
				"cue",
			},
			highlight = {
				enable = true,
				-- additional_vim_regex_highlighting = { "markdown" },
				indent = { enable = false },
			},
			textobjects = {
				select = {
					enable = true,
					lookahead = true,
					include_surrounding_whitespace = false,
					keymaps = {
						["aa"] = "@parameter.outer",
						["ia"] = "@parameter.inner",
						["af"] = "@function.outer",
						["if"] = "@function.inner",
						["ac"] = "@class.outer",
						["ic"] = "@class.inner",
						["ao"] = "@conditional.outer",
						["io"] = "@conditional.inner",
					},
				},
				move = {
					enable = true,
					set_jumps = false,
					goto_next_start = {
						["]f"] = "@function.outer",
						["]a"] = "@parameter.inner",
					},
					goto_previous_start = {
						["[f"] = "@function.outer",
						["[a"] = "@parameter.inner",
					},
				},
			},
			textsubjects = {
				enable = true,
				prev_selection = ",",
				keymaps = {
					["."] = "textsubjects-smart",
					[";"] = "textsubjects-container-outer",
				},
			},
		},
		config = function(plugin, opts)
			setup_cue()
			require("nvim-treesitter.configs").setup(opts)
		end,
	},
}
