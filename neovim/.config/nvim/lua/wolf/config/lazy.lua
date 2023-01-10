local lazypath = vim.fn.stdpath("data") .. "/lazy/lazy.nvim"
local lazyconfig = {
	spec = {
		{ import = "wolf.plugins" },
	},
	install = { colorscheme = { "nordfox", "nightfox" } },
	defaults = {
		lazy = true, -- load every plugin lazily by default
		-- version = "*", -- try installing the latest stable version for plugins that use semver
	},
	checker = { enabled = true, notify = false },
	performance = {
		rtp = {
			disabled_plugins = {
				"gzip",
				"matchit",
				"matchparen",
				"netrwPlugin",
				"tarPlugin",
				"tohtml",
				"tutor",
				"zipPlugin",
			},
		},
	},
}

if vim.g.vscode then
	lazypath = vim.fn.stdpath("data") .. "/vscode/lazy/lazy.nvim"
	lazyconfig.spec = {
		{ import = "wolf.vscode" },
	}
	lazyconfig.install = {}
end

if not vim.loop.fs_stat(lazypath) then
  -- bootstrap lazy.nvim
  -- stylua: ignore
  vim.fn.system({ "git", "clone", "--filter=blob:none", "https://github.com/folke/lazy.nvim.git", "--branch=stable", lazypath })
end
vim.opt.rtp:prepend(vim.env.LAZY or lazypath)

require("lazy").setup(lazyconfig)
