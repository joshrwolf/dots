local M = {}

local Terminal = require("toggleterm.terminal").Terminal

local git = Terminal:new {
  cmd = "lazygit",
  hidden = true,
  direction = "float",
  on_open = function(term)
    vim.cmd("startinsert!")
    vim.api.nvim_buf_set_keymap(term.bufnr, "n", "q", "<cmd>close<cr>", { noremap = true, silent = true })
  end,
  on_close = function(term)
    -- vim.cmd("Closing terminal") 
  end,
}

function M.toggle_git()
  git:toggle()
end

return M
