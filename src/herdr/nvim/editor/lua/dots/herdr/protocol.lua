-- Wire contract paired with herdr-nvim/src/protocol.rs. No legacy shapes.
local M = { version = 1, max_request_bytes = 16384 }

---@alias dots.herdr.ErrorCode 'invalid_request'|'protocol_mismatch'|'not_attached'|'identity_mismatch'|'host_unavailable'|'listener_unavailable'|'invalid_target'|'invalid_position'|'dirty_buffer'|'unsupported_window'|'editor_changed'|'presentation_failed'|'swap_conflict'|'expired'|'not_managed'|'owner_required'|'owner_mismatch'|'owner_expired'|'owner_active'|'internal'
-- Exported for the Rust/Lua contract parity test, not inferred from messages.
M.codes = {
  "invalid_request",
  "protocol_mismatch",
  "not_attached",
  "identity_mismatch",
  "host_unavailable",
  "listener_unavailable",
  "invalid_target",
  "invalid_position",
  "dirty_buffer",
  "unsupported_window",
  "editor_changed",
  "presentation_failed",
  "swap_conflict",
  "expired",
  "not_managed",
  "owner_required",
  "owner_mismatch",
  "owner_expired",
  "owner_active",
  "internal",
}
---@class dots.herdr.Failure
---@field code dots.herdr.ErrorCode
---@field message string
---@class dots.herdr.Context
---@field registration string
---@field server string
---@class dots.herdr.Location
---@field path string
---@field line integer One-based line.
---@field column integer One-based byte column.
---@class dots.herdr.LeaseId
---@field owner string
---@field generation integer
---@class dots.herdr.Operation
---@field method 'hello'|'open'|'owner'
---@field lease? dots.herdr.LeaseId
---@field control? table
---@field target? dots.herdr.Location
---@field not_after_ms? integer Unix epoch milliseconds on this host.
---@class dots.herdr.Request
---@field protocol integer
---@field context dots.herdr.Context
---@field operation dots.herdr.Operation
---@class dots.herdr.Identity
---@field protocol integer
---@field context dots.herdr.Context
---@field pid integer

local failure_mt = {
  __tostring = function(err)
    return err.code .. ": " .. err.message
  end,
}

---@param code dots.herdr.ErrorCode
---@param message string
function M.raise(code, message)
  error(setmetatable({ code = code, message = message }, failure_mt), 0)
end

function M.check(condition, code, message)
  if not condition then
    M.raise(code, message)
  end
end

---@return dots.herdr.Failure
function M.failure(err, fallback)
  if getmetatable(err) == failure_mt then
    return { code = err.code, message = err.message }
  end
  return { code = fallback or "internal", message = tostring(err) }
end

local function object(value, fields, optional)
  M.check(type(value) == "table", "invalid_request", "expected an object")
  for key in pairs(value) do
    M.check(
      type(key) == "string" and (fields[key] or (optional and optional[key])),
      "invalid_request",
      "unknown object field: " .. tostring(key)
    )
  end
  for key in pairs(fields) do
    M.check(value[key] ~= nil and value[key] ~= vim.NIL, "invalid_request", "missing field: " .. key)
  end
end

local function text(value)
  return type(value) == "string" and #value > 0 and not value:find("%c")
end

local function uint(value)
  return type(value) == "number" and value >= 0 and value <= 4294967295 and value == math.floor(value)
end

function M.token(value)
  return type(value) == "string" and #value == 32 and value:match("^[0-9a-f]+$") ~= nil
end

local function lease(value)
  object(value, { owner = true, generation = true })
  M.check(
    M.token(value.owner) and uint(value.generation) and value.generation > 0,
    "invalid_request",
    "invalid owner lease"
  )
end

local function deadline(value)
  M.check(
    type(value) == "number" and value > 0 and value <= 9007199254740991 and value == math.floor(value),
    "invalid_request",
    "invalid operation deadline"
  )
end

local function control(value)
  M.check(type(value) == "table", "invalid_request", "expected owner control")
  if value.action == "inspect" then
    object(value, { action = true })
  elseif value.action == "renew" or value.action == "release" then
    object(value, { action = true, lease = true, not_after_ms = true })
    lease(value.lease)
    deadline(value.not_after_ms)
  elseif value.action == "recover" then
    object(value, { action = true, expected = true, owner = true, not_after_ms = true })
    lease(value.expected)
    M.check(M.token(value.owner), "invalid_request", "invalid new owner")
    deadline(value.not_after_ms)
  else
    M.raise("invalid_request", "unknown owner operation")
  end
end

---@param json string
---@return dots.herdr.Request
function M.decode(json)
  M.check(type(json) == "string" and #json <= M.max_request_bytes, "invalid_request", "invalid request size")
  local ok, request = pcall(vim.json.decode, json)
  M.check(ok, "invalid_request", "request is not JSON")
  object(request, { protocol = true, context = true, operation = true })
  M.check(uint(request.protocol), "invalid_request", "protocol must be an integer")
  M.check(request.protocol == M.version, "protocol_mismatch", "unsupported editor protocol")
  local context = request.context
  object(context, { registration = true, server = true })
  for key, value in pairs(context) do
    M.check(text(value), "invalid_request", "invalid context field: " .. key)
  end
  M.check(M.token(context.registration), "invalid_request", "invalid registration token")
  M.check(context.server:sub(1, 1) == "/", "invalid_request", "server must be an absolute socket path")
  local operation = request.operation
  M.check(type(operation) == "table", "invalid_request", "expected operation object")
  if operation.method == "hello" then
    object(operation, { method = true })
  elseif operation.method == "owner" then
    object(operation, { method = true, control = true })
    control(operation.control)
  elseif operation.method == "open" then
    object(operation, { method = true, target = true, not_after_ms = true }, { lease = true })
    if operation.lease ~= nil then
      lease(operation.lease)
    end
    object(operation.target, { path = true, line = true, column = true })
    deadline(operation.not_after_ms)
    local target = operation.target
    M.check(
      type(target.path) == "string" and target.path:sub(1, 1) == "/" and not target.path:find("\0", 1, true),
      "invalid_target",
      "target must be an absolute path without NUL bytes"
    )
    M.check(
      uint(target.line) and target.line > 0 and uint(target.column) and target.column > 0,
      "invalid_position",
      "coordinates must be positive one-based integers"
    )
  else
    M.raise("invalid_request", "unknown editor operation")
  end
  return request
end

function M.now_ms()
  local seconds, micros = vim.uv.gettimeofday()
  return seconds * 1000 + math.floor(micros / 1000)
end

function M.fresh(not_after_ms)
  M.check(
    M.now_ms() < not_after_ms,
    "expired",
    "request expired; inspect current state before requesting another operation"
  )
end

return M
