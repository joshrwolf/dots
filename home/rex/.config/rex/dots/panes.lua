-- Splitting and resizing panes. Moving focus belongs to dots.nav.
local M = {}

function M.install()
  rex.bind("ctrl+s>shift+\\", "pane.split", { direction = "right" })
  rex.bind("ctrl+s>-", "pane.split", { direction = "down" })
  rex.bind("ctrl+shift+h", "pane.resize", { direction = "left" })
  rex.bind("ctrl+shift+j", "pane.resize", { direction = "down" })
  rex.bind("ctrl+shift+k", "pane.resize", { direction = "up" })
  rex.bind("ctrl+shift+l", "pane.resize", { direction = "right" })
end

return M
