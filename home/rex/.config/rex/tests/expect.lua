-- Assertions for tests/run.lua. Failures name both values; the runner names
-- the test, since gopher-lua misreports error levels.
local M = {}

local function show(value)
  if type(value) == "table" then
    local encoded = rex.json.encode(value)
    return encoded or tostring(value)
  end
  return string.format("%q", tostring(value))
end

local function same(a, b)
  if type(a) ~= "table" or type(b) ~= "table" then
    return a == b
  end
  for k, v in pairs(a) do
    if not same(v, b[k]) then
      return false
    end
  end
  for k in pairs(b) do
    if a[k] == nil then
      return false
    end
  end
  return true
end

---@param actual any
---@param expected any
function M.equal(actual, expected)
  if not same(actual, expected) then
    error("expected " .. show(expected) .. ", got " .. show(actual), 0)
  end
end

---@param fn function
---@param pattern string A plain substring the error message must contain.
function M.fails(fn, pattern)
  local ok, err = pcall(fn)
  if ok then
    error("expected an error containing " .. show(pattern), 0)
  end
  if not tostring(err):find(pattern, 1, true) then
    error("expected an error containing " .. show(pattern) .. ", got " .. show(err), 0)
  end
end

return M
