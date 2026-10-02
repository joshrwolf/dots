-- Catch external edits when returning to files, even if a file watcher missed them.
-- Default autoread reloads clean buffers; checktime protects unsaved edits.
vim.api.nvim_create_autocmd({ "FocusGained", "BufEnter", "TermClose", "TermLeave" }, {
  group = vim.api.nvim_create_augroup("dots.checktime", { clear = true }),
  callback = function()
    if vim.bo.buftype == "" then
      vim.cmd.checktime()
    end
  end,
})

-- Restore once on ordinary file opens, never on reloads or hidden buffer loads.
-- Keep this synchronous so explicit destinations (Herdr, +line, search) win.
vim.api.nvim_create_autocmd("BufWinEnter", {
  group = vim.api.nvim_create_augroup("dots.last_position", { clear = true }),
  callback = function(event)
    if vim.b[event.buf].dots_last_position then
      return
    end
    vim.b[event.buf].dots_last_position = true
    local buffer = vim.bo[event.buf]
    if
      event.buf ~= vim.api.nvim_get_current_buf()
      or buffer.buftype ~= ""
      or buffer.filetype == "gitcommit"
      or buffer.filetype == "gitrebase"
    then
      return
    end
    local mark = vim.api.nvim_buf_get_mark(event.buf, '"')
    if mark[1] > 0 and mark[1] <= vim.api.nvim_buf_line_count(event.buf) then
      vim.api.nvim_win_set_cursor(0, mark)
    end
  end,
})

vim.api.nvim_create_autocmd("TextYankPost", {
  group = vim.api.nvim_create_augroup("dots.highlight_yank", { clear = true }),
  callback = function()
    -- hl_op replaces on_yank in Neovim 0.13.
    (vim.hl.hl_op or vim.hl.on_yank)()
  end,
})

-- Saving a new local file should also create its missing parent directories.
vim.api.nvim_create_autocmd("BufWritePre", {
  group = vim.api.nvim_create_augroup("dots.create_parent", { clear = true }),
  callback = function(event)
    if vim.bo[event.buf].buftype ~= "" or event.match:match("^%w%w+:[\\/][\\/]") then
      return
    end
    local path = vim.uv.fs_realpath(event.match) or event.match
    vim.fn.mkdir(vim.fn.fnamemodify(path, ":p:h"), "p")
  end,
})
