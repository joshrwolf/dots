return {
	{ "folke/which-key.nvim" },
	{
		"christoomey/vim-tmux-navigator",
		event = "VeryLazy",
		config = function()
			vim.g.tmux_navigator_save_on_switch = 1
		end,
	},
	{ "ThePrimeagen/harpoon" },
	{
		"tpope/vim-fugitive",
		cmd = { "Git" },
    config = function ()
      vim.api.nvim_create_autocmd("FileType", {
        pattern = "fugitive",
        callback =function (ctx)
          vim.keymap.set("n", "<Tab>", "=", { remap = true,  buffer = ctx.buf })
          vim.keymap.set("n", "q", ":q<cr>", { buffer = ctx.buf })

          vim.keymap.set("n", "dt", ":Gtabedit <Plug><cfile><Bar>Gdiffsplit<CR>", { buffer = ctx.buf })
        end
      })
    end
	},
	{
		"RRethy/vim-illuminate",
		event = "VeryLazy",
		config = function()
			require("illuminate").configure({
				providers = {
					"lsp",
					"treesitter",
					"regexp",
				},
			})
		end,
	},
	{
		"epwalsh/obsidian.nvim",
		cmd = { "ObsidianNew", "ObsidianOpen", "ObsidianToday", "ObsidianYesterday", "ObsidianSearch" },
		config = function()
			require("obsidian").setup({
				dir = "~/.brain",
				completion = {
					nvim_cmp = true,
				},
				daily_notes = { folder = "dailies" },
			})
		end,
	},
	{
		"m-demare/hlargs.nvim",
		event = "VeryLazy",
		config = function()
			local nf = require("nightfox.palette").load("nordfox")
			require("hlargs").setup({
				color = nf.white.dim,
			})
		end,
	},
}
