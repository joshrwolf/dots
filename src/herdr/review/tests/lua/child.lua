-- Minimal isolated editor: never load user configuration or contact live Herdr.
local config = assert(vim.env.HERDR_REVIEW_CONFIG)
local tests = assert(vim.env.HERDR_REVIEW_TESTS)
package.path = config .. "/lua/?.lua;" .. config .. "/lua/?/init.lua;" .. tests .. "/lua/?.lua;" .. package.path
vim.env.HERDR_ENV = nil
vim.env.HERDR_PANE_ID = nil
vim.env.HERDR_SOCKET_PATH = "/tmp/example-herdr.sock"
-- Use real swap behavior; both editors in a conflict test share this directory.
vim.o.directory = assert(vim.env.HERDR_REVIEW_SCRATCH) .. "/swap//"
vim.fn.mkdir(vim.o.directory, "p", 448)
vim.env.NVIM = nil
_G.test_root = assert(vim.env.HERDR_REVIEW_SCRATCH) .. "/case-" .. vim.fn.getpid()
vim.fn.mkdir(test_root, "p", 448)
vim.api.nvim_create_autocmd("VimLeavePre", {
  callback = function()
    vim.fn.delete(test_root, "rf")
  end,
})
