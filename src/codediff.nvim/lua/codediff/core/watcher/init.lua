local M = {}

local process = require("codediff.core.watcher.process")

local repositories = {}
local next_subscriber_id = 0

local function notify(entry, event, ...)
  local subscribers = {}
  for _, subscriber in pairs(entry.subscribers) do
    subscribers[#subscribers + 1] = subscriber
  end
  for _, subscriber in ipairs(subscribers) do
    local callback = subscriber[event]
    if callback then
      pcall(callback, ...)
    end
  end
end

local function start(entry)
  local watcher, start_error = process.start(entry.repository, {
    on_ready = function(message)
      if repositories[entry.repository] ~= entry then
        return
      end
      entry.ready = true
      entry.ready_message = message
      notify(entry, "on_ready", message)
    end,
    on_refresh = function(message)
      if repositories[entry.repository] == entry then
        notify(entry, "on_refresh", message)
      end
    end,
    on_error = function(message)
      if repositories[entry.repository] ~= entry then
        return
      end
      entry.process = nil
      entry.ready = false
      entry.ready_message = nil
      entry.error = message
      notify(entry, "on_error", message)
    end,
  })
  if not watcher then
    vim.notify("CodeDiff: " .. start_error .. "; using polling refresh", vim.log.levels.WARN)
    entry.error = start_error
    notify(entry, "on_error", start_error)
    return
  end
  entry.process = watcher
end

function M.stop_all()
  local active = repositories
  repositories = {}
  for _, entry in pairs(active) do
    if entry.process then
      entry.process.stop()
      entry.process = nil
    end
  end
end

function M.subscribe(repository, handlers)
  handlers = handlers or {}
  next_subscriber_id = next_subscriber_id + 1
  local subscriber_id = next_subscriber_id
  local entry = repositories[repository]
  if not entry then
    entry = {
      repository = repository,
      subscribers = {},
      process = nil,
      ready = false,
      ready_message = nil,
      error = nil,
    }
    repositories[repository] = entry
  end
  entry.subscribers[subscriber_id] = handlers

  if entry.ready and handlers.on_ready then
    handlers.on_ready(entry.ready_message)
  elseif entry.error and handlers.on_error then
    handlers.on_error(entry.error)
  elseif not entry.process then
    start(entry)
  end

  local subscribed = true
  return function()
    if not subscribed then
      return
    end
    subscribed = false
    local current = repositories[repository]
    if current ~= entry then
      return
    end
    entry.subscribers[subscriber_id] = nil
    if next(entry.subscribers) ~= nil then
      return
    end
    repositories[repository] = nil
    if entry.process then
      entry.process.stop()
      entry.process = nil
    end
  end
end

local group = vim.api.nvim_create_augroup("CodeDiffNativeWatcher", { clear = true })
vim.api.nvim_create_autocmd("VimLeavePre", {
  group = group,
  callback = M.stop_all,
})

return M
