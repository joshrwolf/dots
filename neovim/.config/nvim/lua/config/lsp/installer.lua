local M = {}

local util = require("lspconfig/util")
local path = util.path

local function get_python_path(workspace)
	-- Use activated virtualenv.
	if vim.env.VIRTUAL_ENV then
		return path.join(vim.env.VIRTUAL_ENV, "bin", "python")
	end

	-- -- Find and use virtualenv from pipenv in workspace directory.
	-- local match = vim.fn.glob(path.join(workspace, 'Pipfile'))
	-- if match ~= '' then
	--   local venv = vim.fn.trim(vim.fn.system('PIPENV_PIPFILE=' .. match .. ' pipenv --venv'))
	--   return path.join(venv, 'bin', 'python')
	-- end

	-- Find and use virtualenv in workspace directory.
	for _, pattern in ipairs({ "*", ".*" }) do
		local match = vim.fn.glob(path.join(workspace, pattern, "pyvenv.cfg"))
		if match ~= "" then
			return path.join(path.dirname(match), "bin", "python")
		end
	end

	-- Fallback to system Python.
	return vim.fn.exepath("python3") or vim.fn.exepath("python") or "python"
end

function M.setup(servers, options)
	local lspconfig = require("lspconfig")

	require("mason").setup()
	require("mason-lspconfig").setup({
		automatic_installation = true,
	})

	-- lsp config as json
	require("nlspsettings").setup({
		config_home = vim.fn.stdpath("config") .. "/nlsp-settings",
		local_settings_dir = ".nlsp-settings",
		local_settings_root_markers = { ".git" },
		append_default_schemas = true,
		loader = "json",
	})

	for server_name, _ in pairs(servers) do
		local opts = vim.tbl_deep_extend("force", options, servers[server_name] or {})

		if server_name == "sumneko_lua" then
			local luadev = require("lua-dev").setup({ lspconfig = opts })
			lspconfig[server_name].setup(luadev)
		elseif server_name == "rust_analyzer" then
			require("rust-tools").setup({
				server = {
					capabilities = opts.capabilities,
					on_attach = opts.on_attach,
				},
        tools = {},
			})
		elseif server_name == "pyright" then
			opts.on_init = function(client)
				client.config.settings.python.pythonPath = get_python_path(client.config.root_dir)
				print("root_dir" .. client.config.root_dir)
				print("path" .. get_python_path(client.config.root_dir))
			end
			lspconfig[server_name].setup(opts)
		else
			lspconfig[server_name].setup(opts)
		end
	end
end

return M
