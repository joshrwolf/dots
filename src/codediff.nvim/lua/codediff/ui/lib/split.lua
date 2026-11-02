-- Drop-in replacement for nui.split
-- Creates split windows with buffer/window option management using native Neovim APIs

local Split = {}
Split.__index = Split

-- Map nui position names to nvim_open_win split values
local POSITION_MAP = {
  left = "left",
  right = "right",
  top = "above",
  bottom = "below",
}

function Split.new(opts)
  local self = setmetatable({}, Split)
  self._opts = opts or {}
  self._position = POSITION_MAP[self._opts.position or "left"] or "left"
  self._size = self._opts.size or 40
  self._buf_options = self._opts.buf_options or {}
  self._win_options = self._opts.win_options or {}
  self.bufnr = nil
  self.winid = nil
  return self
end

-- Allow Split({...}) constructor syntax (matches NuiSplit)
setmetatable(Split, {
  __call = function(cls, opts)
    return cls.new(opts)
  end,
})

function Split:_apply_buf_options()
  if not self.bufnr or not vim.api.nvim_buf_is_valid(self.bufnr) then
    return
  end
  for k, v in pairs(self._buf_options) do
    pcall(function()
      vim.bo[self.bufnr][k] = v
    end)
  end
end

function Split:_apply_win_options()
  if not self.winid or not vim.api.nvim_win_is_valid(self.winid) then
    return
  end
  for k, v in pairs(self._win_options) do
    pcall(function()
      vim.wo[self.winid][k] = v
    end)
  end
end

function Split:_create_window()
  local win_config = {
    split = self._position,
    win = -1, -- split relative to editor (not current window)
  }
  if self._position == "left" or self._position == "right" then
    win_config.width = self._size
  else
    win_config.height = self._size
  end

  self.winid = vim.api.nvim_open_win(self.bufnr, false, win_config)
  self:_apply_win_options()

  -- Force size after creation (wincmd = from callers can override the config size)
  if self.winid and vim.api.nvim_win_is_valid(self.winid) then
    if self._position == "left" or self._position == "right" then
      vim.api.nvim_win_set_width(self.winid, self._size)
    else
      vim.api.nvim_win_set_height(self.winid, self._size)
    end
  end
end

--- Mount the split: create buffer and window
function Split:mount()
  self.bufnr = vim.api.nvim_create_buf(false, true)
  self:_apply_buf_options()
  self:_create_window()
end

--- Show a previously hidden split (re-creates window with same buffer)
function Split:show()
  if not self.bufnr or not vim.api.nvim_buf_is_valid(self.bufnr) then
    return
  end
  if self.winid and vim.api.nvim_win_is_valid(self.winid) then
    return
  end
  self:_create_window()
end

--- Hide the split (closes window, preserves buffer)
function Split:hide()
  if self.winid and vim.api.nvim_win_is_valid(self.winid) then
    vim.api.nvim_win_hide(self.winid)
    self.winid = nil
  end
end

return Split
