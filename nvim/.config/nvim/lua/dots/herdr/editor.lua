-- Editor behavior only: real buffers, windows and autocommands. No Herdr I/O.
local P = require("dots.herdr.protocol")
local M = {}

local function ordinary(window)
  return vim.api.nvim_win_is_valid(window)
    and vim.api.nvim_win_get_config(window).relative == ""
    and vim.bo[vim.api.nvim_win_get_buf(window)].buftype == ""
end

local function destination()
  local current = vim.api.nvim_get_current_win()
  if ordinary(current) then
    return current
  end
  local alternate = vim.fn.win_getid(vim.fn.winnr("#"))
  if ordinary(alternate) then
    return alternate
  end
  for _, window in ipairs(vim.api.nvim_tabpage_list_wins(0)) do
    if ordinary(window) then
      return window
    end
  end
  P.raise("unsupported_window", "no ordinary file window in this Neovim tab")
end

local function load(buffer)
  -- E325 can leave a loaded buffer behind. Remember the conflict even when a
  -- retry would skip bufload; only explicit user disposal/recovery may bypass it.
  local function conflict()
    P.raise("swap_conflict", "resolve the swap conflict in Neovim and wipe/reopen this buffer before retrying")
  end
  if vim.b[buffer].dots_herdr_swap_conflict then
    conflict()
  end
  if vim.api.nvim_buf_is_loaded(buffer) then
    return
  end
  local collided = false
  local handler = vim.api.nvim_create_autocmd("SwapExists", {
    buffer = buffer,
    callback = function()
      collided = true
      vim.v.swapchoice = "q" -- Never recover, delete, or edit someone else's swap.
    end,
  })
  local ok, err = pcall(vim.fn.bufload, buffer)
  -- User hooks may have removed it; cleanup must not mask a recorded conflict.
  pcall(vim.api.nvim_del_autocmd, handler)
  if collided or (not ok and tostring(err):find("E325", 1, true)) then
    if vim.api.nvim_buf_is_valid(buffer) then
      vim.b[buffer].dots_herdr_swap_conflict = true
    end
    conflict()
  end
  if not ok then
    error(err, 0)
  end
end

---@param target dots.herdr.Location Validated wire location.
---@param revalidate fun() Check local identity and expiry, without I/O.
---@return dots.herdr.Failure? partial A failure after presentation began.
---@return dots.herdr.Location? observed Actual buffer name and cursor.
function M.open(target, revalidate)
  revalidate()
  local stat = vim.uv.fs_stat(target.path)
  P.check(stat and stat.type == "file", "invalid_target", "target is not a regular file")
  local expected_path = vim.uv.fs_realpath(target.path)
  P.check(expected_path, "invalid_target", "target disappeared while resolving it")
  local window, origin = destination(), vim.api.nvim_get_current_win()
  local previous = vim.api.nvim_win_get_buf(window)
  revalidate()
  local buffer = vim.fn.bufadd(target.path)
  local function can_leave()
    return buffer == previous
      or not vim.bo[previous].modified
      or vim.bo[previous].bufhidden == "hide"
      or (vim.o.hidden and vim.bo[previous].bufhidden == "")
  end
  P.check(can_leave(), "dirty_buffer", "destination buffer has unsaved changes")
  revalidate()
  load(buffer)
  P.check(vim.api.nvim_buf_is_loaded(buffer), "invalid_target", "could not load target buffer")
  local text = vim.api.nvim_buf_get_lines(buffer, target.line - 1, target.line, false)[1]
  local byte = text and text:byte(target.column)
  P.check(
    text ~= nil and target.column <= math.max(1, #text) and (not byte or byte < 128 or byte >= 192),
    "invalid_position",
    "position is outside the buffer or inside a UTF-8 sequence"
  )
  P.check(
    ordinary(window)
      and vim.api.nvim_get_current_win() == origin
      and vim.api.nvim_win_get_buf(window) == previous
      and vim.uv.fs_realpath(vim.api.nvim_buf_get_name(buffer)) == expected_path,
    "editor_changed",
    "window or target changed while loading"
  )
  P.check(can_leave(), "dirty_buffer", "destination buffer changed while loading target")
  revalidate()
  -- Arbitrary user hooks cannot be cancelled or rolled back. Once presentation
  -- starts, report failures as partial and never claim that nothing happened.
  local observed
  local ok, err = pcall(function()
    vim.bo[buffer].buflisted = true
    vim.api.nvim_set_current_win(window)
    P.check(
      vim.api.nvim_win_get_buf(window) == previous and can_leave(),
      "editor_changed",
      "destination changed on focus"
    )
    revalidate()
    vim.api.nvim_win_set_buf(window, buffer)
    P.check(
      vim.api.nvim_get_current_win() == window and vim.api.nvim_win_get_buf(window) == buffer,
      "presentation_failed",
      "window changed while opening target"
    )
    revalidate()
    vim.api.nvim_win_set_cursor(window, { target.line, target.column - 1 })
    local cursor = vim.api.nvim_win_get_cursor(window)
    observed = { path = vim.api.nvim_buf_get_name(buffer), line = cursor[1], column = cursor[2] + 1 }
    P.check(
      vim.uv.fs_realpath(observed.path) == expected_path
        and observed.line == target.line
        and observed.column == target.column,
      "presentation_failed",
      "buffer/cursor did not reach target"
    )
  end)
  if not ok then
    return P.failure(err, "presentation_failed")
  end
  return nil, observed
end

return M
