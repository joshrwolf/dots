-- Autocmds are automatically loaded on the VeryLazy event
-- Default autocmds that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/autocmds.lua
-- Add any additional autocmds here

-- show cursor line only in active window
vim.api.nvim_create_autocmd({ "InsertLeave", "WinEnter" }, {
  callback = function()
    local ok, cl = pcall(vim.api.nvim_win_get_var, 0, "auto-cursorline")
    if ok and cl then
      vim.wo.cursorline = true
      vim.api.nvim_win_del_var(0, "auto-cursorline")
    end
  end,
})
vim.api.nvim_create_autocmd({ "InsertEnter", "WinLeave" }, {
  callback = function()
    local cl = vim.wo.cursorline
    if cl then
      vim.api.nvim_win_set_var(0, "auto-cursorline", cl)
      vim.wo.cursorline = false
    end
  end,
})

-- Don't auto comment new line
vim.api.nvim_create_autocmd("BufEnter", { command = [[set formatoptions-=cro]] })

-- CUE-specific settings: use tabs instead of spaces
vim.api.nvim_create_autocmd("FileType", {
  pattern = "cue",
  callback = function()
    vim.bo.expandtab = false -- Use tabs instead of spaces
    vim.bo.tabstop = 4 -- Display tabs as 4 spaces wide
    vim.bo.shiftwidth = 4 -- Use 4 spaces for indentation commands
    vim.bo.softtabstop = 4 -- Insert/delete 4 spaces when hitting Tab/Backspace
  end,
})
