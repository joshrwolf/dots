local M = {}

local nls = require "null-ls"

local sources = {}

function M.setup(opts)
  nls.setup {
    debounce = 150,
    save_after_format = false,
  }
end

return M
