local M = {
	"nvim-treesitter/nvim-treesitter",
	build = ":TSUpdate",
	event = "BufReadPost",

	dependencies = {
		"nvim-treesitter/nvim-treesitter-textobjects",
		"RRethy/nvim-treesitter-textsubjects",
		{ "nvim-treesitter/playground", cmd = "TSPlaygroundToggle" },
	},
}

function M.config()
	require("nvim-treesitter.configs").setup({
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
			"lua",
			"markdown",
			"markdown_inline",
			"nix",
			"python",
			"regex",
			"rust",
			"scss",
			"sql",
			"toml",
			"tsx",
			"terraform",
			"typescript",
			"vim",
			"yaml",
		},
		highlight = { enable = true },
		indent = { enable = true },
		incremental_selection = {
			enable = true,
			keymaps = {
				init_selection = "<C-space>",
				node_incremental = "<C-space>",
				scope_incremental = "<C-s>",
				node_decremental = "<C-backspace>",
			},
		},
		textobjects = {
			select = {
				enable = true,
				lookahead = true,
				keymaps = {
					["aa"] = "@parameter.out",
					["ia"] = "@parameter.inner",
					["af"] = "@function.outer",
					["if"] = "@function.inner",
					["ac"] = "@class.outer",
					["ic"] = "@class.inner",
				},
			},
			move = {
				enable = true,
				set_jumps = false,
				goto_next_start = {
					["]f"] = "@function.outer",
				},
				goto_next_end = {
					["]F"] = "@function.outer",
				},
				goto_previous_start = {
					["[f"] = "@function.outer",
				},
				goto_previous_end = {
					["[F"] = "@function.outer",
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
	})
end

return M
