-- Reuse the caller-aware Herdr adapter; inherited pane/workspace IDs can be
-- stale after a move. Socket identity is a cache invalidation hint, not authority.
local M = {}

function M.current()
  local server = vim.env.HERDR_SOCKET_PATH
  if vim.env.HERDR_ENV ~= "1" or not server or server == "" then
    return nil, { code = "runtime_unavailable", message = "review commands require Herdr" }
  end
  local ok, pane = pcall(function()
    return require("dots.herdr.host").new(server).current()
  end)
  if not ok then
    return nil, { code = "runtime_unavailable", message = type(pane) == "table" and pane.message or tostring(pane) }
  end
  local stat, err = vim.uv.fs_stat(server)
  if not stat or stat.type ~= "socket" then
    return nil, { code = "runtime_unavailable", message = err or "Herdr endpoint is not a socket" }
  end
  local env = {
    HERDR_ENV = "1",
    HERDR_SOCKET_PATH = server,
    HERDR_WORKSPACE_ID = pane.workspace,
    HERDR_TAB_ID = pane.tab,
    HERDR_PANE_ID = pane.pane,
  }
  return {
    env = env,
    key = vim.json.encode({ server, stat.dev, stat.ino, pane.workspace, pane.tab, pane.pane }),
  }
end

return M
