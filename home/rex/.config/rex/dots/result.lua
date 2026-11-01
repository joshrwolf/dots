-- Rex API calls return `value, err`. Actions turn an unexpected failure into
-- an error whose message reaches whoever ran them, without a file:line prefix.
local M = {}

---Return `value`, or raise `err` as the caller's failure. Pass a call as the
---last argument so both of its results arrive:
---`must("reading the session", rex.session.view({}))`.
---@generic T
---@param context string What was being attempted.
---@param value T?
---@param err? string
---@return T
function M.must(context, value, err)
  if value == nil then
    error(context .. ": " .. (err or "unknown error"), 0)
  end
  return value
end

return M
