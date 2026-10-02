local T, child = require("support").child_set()
local eq = MiniTest.expect.equality
local function request()
  return { protocol = 1, context = require("support").context(), operation = { method = "hello" } }
end
local function decode(json)
  return child.lua_func(function(raw)
    local P = require("dots.herdr.protocol")
    local ok, result = pcall(P.decode, raw)
    return ok and "ok" or P.failure(result).code
  end, json)
end

T["valid hello and open shapes"] = function()
  eq(decode(vim.json.encode(request())), "ok")
  local value = request()
  value.operation =
    { method = "open", not_after_ms = 1, target = { path = "/tmp/quoted 'λ.txt", line = 1, column = 1 } }
  eq(decode(vim.json.encode(value)), "ok")
end

T["invalid envelopes"] = MiniTest.new_set({ parametrize = {
  { "not JSON" },
  { "null" },
  { "[]" },
  { "{}" },
} })
T["invalid envelopes"]["fail with a stable code"] = function(raw)
  eq(decode(raw), "invalid_request")
end

T["oversized requests are rejected"] = function()
  eq(decode(string.rep("x", 16385)), "invalid_request")
end

T["unknown fields are rejected instead of silently ignored"] = function()
  local value = request()
  value.extra = true
  eq(decode(vim.json.encode(value)), "invalid_request")
  value = request()
  value.context.extra = true
  eq(decode(vim.json.encode(value)), "invalid_request")
  value = request()
  value.operation.pid = 42
  eq(decode(vim.json.encode(value)), "invalid_request")
end

T["protocol mismatch is distinct from malformed protocol"] = function()
  local value = request()
  value.protocol = 99
  eq(decode(vim.json.encode(value)), "protocol_mismatch")
  value.protocol = "1"
  eq(decode(vim.json.encode(value)), "invalid_request")
end

T["invalid coordinates"] =
  MiniTest.new_set({ parametrize = {
    { 0 },
    { -1 },
    { 1.5 },
    { 4294967296 },
    { "2" },
  } })
T["invalid coordinates"]["are rejected before editor calls"] = function(line)
  local value = request()
  value.operation = { method = "open", not_after_ms = 1, target = { path = "/tmp/file", line = line, column = 1 } }
  eq(decode(vim.json.encode(value)), "invalid_position")
end

T["tokens, identities and paths are validated"] = function()
  local value = request()
  value.context.registration = "../escape"
  eq(decode(vim.json.encode(value)), "invalid_request")
  value = request()
  value.context.server = vim.NIL
  eq(decode(vim.json.encode(value)), "invalid_request")
  value = request()
  value.operation = { method = "open", not_after_ms = 1, target = { path = "relative", line = 1, column = 1 } }
  eq(decode(vim.json.encode(value)), "invalid_target")
end
T["invalid deadlines"] = MiniTest.new_set({ parametrize = { { 0 }, { -1 }, { 1.5 }, { "1" }, { 9007199254740992 } } })
T["invalid deadlines"]["are rejected"] = function(deadline)
  local value = request()
  value.operation = { method = "open", not_after_ms = deadline, target = { path = "/tmp/file", line = 1, column = 1 } }
  eq(decode(vim.json.encode(value)), "invalid_request")
end

T["optional owner fences reject null and unknown fields"] = function()
  local value = request()
  value.operation = {
    method = "open",
    target = { path = "/tmp/file", line = 1, column = 1 },
    not_after_ms = 1,
    lease = { owner = string.rep("a", 32), generation = 1 },
  }
  eq(decode(vim.json.encode(value)), "ok")
  value.operation.lease.extra = true
  eq(decode(vim.json.encode(value)), "invalid_request")
  value.operation.lease = vim.NIL
  eq(decode(vim.json.encode(value)), "invalid_request")
end

T["read-only ownership inspection accepts no mutation fields"] = function()
  local value = request()
  value.operation = { method = "owner", control = { action = "inspect" } }
  eq(decode(vim.json.encode(value)), "ok")
  value.operation.control.not_after_ms = 1
  eq(decode(vim.json.encode(value)), "invalid_request")
end

return T
