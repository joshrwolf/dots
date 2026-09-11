local M = {}

local function body(bufnr)
  local lines = vim.api.nvim_buf_get_lines(bufnr, 0, -1, false)
  while #lines > 0 and not lines[1]:match("%S") do
    table.remove(lines, 1)
  end
  while #lines > 0 and not lines[#lines]:match("%S") do
    table.remove(lines)
  end
  return table.concat(lines, "\n")
end

---Open a disposable Markdown buffer for one review message.
---@param coordinate table
---@param callback fun(body: string)
function M.open(coordinate, callback)
  local bufnr = vim.api.nvim_create_buf(false, true)
  vim.bo[bufnr].buftype = "acwrite"
  vim.bo[bufnr].bufhidden = "wipe"
  vim.bo[bufnr].swapfile = false
  vim.bo[bufnr].filetype = "markdown"
  vim.api.nvim_buf_set_name(bufnr, string.format("herdr-review://message/%d", bufnr))

  local parent = vim.api.nvim_get_current_win()
  local parent_width = vim.api.nvim_win_get_width(parent)
  local parent_height = vim.api.nvim_win_get_height(parent)
  local available_width = math.max(1, parent_width - 4)
  local available_height = math.max(1, parent_height - 2)
  local width = math.min(available_width, math.max(32, math.min(80, math.floor(parent_width * 0.72))))
  local height = math.min(available_height, 7)
  local cursor_row = vim.fn.winline()
  local cursor_col = vim.fn.wincol() - 1
  local below = cursor_row + height + 1 <= parent_height
  local title = string.format(" Review %s:%d ", coordinate.path, coordinate.start_line)
  if vim.fn.strdisplaywidth(title) > math.max(1, width - 2) then
    title = vim.fn.strcharpart(title, 0, math.max(1, width - 5)) .. "…"
  end
  local window_options = {
    relative = "win",
    win = parent,
    row = below and cursor_row or math.max(0, cursor_row - height - 1),
    col = math.max(0, math.min(cursor_col, parent_width - width - 2)),
    width = width,
    height = height,
    style = "minimal",
    border = "rounded",
    zindex = 60,
  }
  if width >= 8 then
    window_options.title = title
    window_options.title_pos = "center"
  end
  if width >= 32 then
    window_options.footer = " :w save · <C-c>/q cancel "
    window_options.footer_pos = "center"
  end
  local win = vim.api.nvim_open_win(bufnr, true, window_options)
  vim.wo[win].wrap = true
  vim.wo[win].linebreak = true

  local finished = false
  local function close()
    if vim.api.nvim_win_is_valid(win) then
      vim.api.nvim_win_close(win, true)
    elseif vim.api.nvim_buf_is_valid(bufnr) then
      vim.api.nvim_buf_delete(bufnr, { force = true })
    end
  end
  local function submit()
    if finished then
      return
    end
    local message = body(bufnr)
    if message == "" then
      vim.notify("review: message body is empty", vim.log.levels.WARN)
      return
    end
    finished = true
    close()
    callback(message)
  end
  local function cancel()
    if finished then
      return
    end
    finished = true
    close()
  end

  vim.keymap.set({ "n", "i" }, "<C-s>", submit, { buffer = bufnr, desc = "Save review message" })
  vim.keymap.set("n", "ZZ", submit, { buffer = bufnr, desc = "Save review message" })
  vim.keymap.set("n", "q", cancel, { buffer = bufnr, desc = "Cancel review message" })
  vim.keymap.set("i", "<C-c>", cancel, { buffer = bufnr, desc = "Cancel review message" })
  vim.api.nvim_create_autocmd("BufWriteCmd", {
    buffer = bufnr,
    callback = submit,
  })
  vim.cmd.startinsert()
end

return M
