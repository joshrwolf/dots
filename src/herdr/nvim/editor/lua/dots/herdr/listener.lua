-- A listener is local editor identity, not a Herdr pane or a daemon owner.
local P = require("dots.herdr.protocol")
local M = {}

---@class dots.herdr.Listener
---@field token string
---@field socket string
---@field close fun()

---@param directory? string Private runtime directory.
---@param token? string Identity durably reserved before a managed launch.
---@return dots.herdr.Listener
function M.open(directory, token)
  local uv = vim.uv
  directory = directory or ("/tmp/dots-nvim-" .. uv.getuid())
  P.check(directory:sub(1, 1) == "/", "listener_unavailable", "socket directory must be absolute")
  P.check(token == nil or P.token(token), "listener_unavailable", "invalid reserved editor identity")
  local stat = uv.fs_lstat(directory)
  if not stat then
    local ok, err = uv.fs_mkdir(directory, 448) -- 0700
    stat = uv.fs_lstat(directory)
    P.check(ok or stat, "listener_unavailable", tostring(err))
  end
  P.check(
    stat and stat.type == "directory" and stat.uid == uv.getuid() and bit.band(stat.mode, 63) == 0,
    "listener_unavailable",
    "editor socket directory must be private and owned by this user"
  )
  if not token then
    local random, err = uv.random(16)
    P.check(random, "listener_unavailable", tostring(err))
    token = random:gsub(".", function(byte)
      return ("%02x"):format(byte:byte())
    end)
  end
  local socket = directory .. "/" .. token .. ".sock"
  -- A stale pathname is not proof of a dead editor. The controller must
  -- reconcile the reservation; never unlink or choose another identity here.
  local existing, stat_error, code = uv.fs_lstat(socket)
  P.check(
    not existing and code == "ENOENT",
    "listener_unavailable",
    existing and "reserved editor path already exists" or tostring(stat_error)
  )
  local ok, started = pcall(vim.fn.serverstart, socket)
  P.check(ok and started == socket, "listener_unavailable", "could not start editor listener: " .. tostring(started))
  local closed = false
  return {
    token = token,
    socket = socket,
    close = function()
      if not closed then
        closed = true
        vim.fn.serverstop(socket)
      end
    end,
  }
end

return M
