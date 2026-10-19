-- Temporary CodeDiff v4 workaround: explorer.position supports only left/bottom.
-- Keep "left" for upstream's width calculations, but move the explorer before
-- each layout pass so reopening it and changing diff layouts retain the right side.
-- This wraps private APIs; recheck on CodeDiff updates and remove when upstream
-- supports explorer.position = "right". No installed plugin files are modified.
local layout = require("codediff.ui.layout")
local lifecycle = require("codediff.ui.lifecycle")
local arrange = layout.arrange

layout.arrange = function(tabpage)
  local session = lifecycle.get_session(tabpage)
  local panel = session and session.panel
  local view = panel and panel.name == "explorer" and panel.view
  local win = view and view.winid
  if win and vim.api.nvim_win_is_valid(win) and not view.is_hidden then
    local col = vim.api.nvim_win_get_position(win)[2]
    if col + vim.api.nvim_win_get_width(win) < vim.o.columns then
      vim.api.nvim_win_call(win, function()
        vim.cmd("wincmd L")
      end)
    end
  end
  return arrange(tabpage)
end
