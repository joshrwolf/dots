-- Tabs (Rex windows), the sidebar that lists them, and its activity panel.
-- H/K and J/L both step through tabs.
local M = {}

function M.install()
  rex.bind("cmd+h", "client.tab.previous")
  rex.bind("cmd+k", "client.tab.previous")
  rex.bind("cmd+j", "client.tab.next")
  rex.bind("cmd+l", "client.tab.next")
  rex.bind("cmd+shift+h", "client.tab.move.backward")
  rex.bind("cmd+shift+l", "client.tab.move.forward")
  rex.bind("cmd+b", "client.sidebar.toggle")
  rex.bind("cmd+shift+a", "client.sidebar.activity.toggle")
end

return M
