-- Asking the Rex app to do something only a client can, such as showing a
-- session. Requires Remote Control in the app's settings.
local must = require("dots.result").must

local M = {}

---The app client to ask: one attached to `session_id` if there is one, else
---the first app connected.
---@param clients table[] `client.list` entries.
---@param session_id? string
---@param preferred? string The app that opened the view, not its CLI connection.
---@return string? client_id
function M.choose(clients, session_id, preferred)
  if preferred then
    local found = false
    for _, client in ipairs(clients) do
      if client.client_id == preferred then
        found = true
        if client.info and client.info.kind == "app" then
          return preferred
        end
      end
    end
    if not found then
      return nil
    end
  end
  local first
  for _, client in ipairs(clients) do
    if client.info and client.info.kind == "app" then
      first = first or client.client_id
      for _, attached in ipairs(client.session_ids or {}) do
        if attached == session_id then
          return client.client_id
        end
      end
    end
  end
  return first
end

---Perform one of the app's own actions.
---@param action string Such as `session.select`.
---@param args? table
---@param near_session? string Prefer the app showing this session.
---@param preferred? string
function M.perform(action, args, near_session, preferred)
  local listed = must("listing clients", rex.call("client.list"))
  local client = M.choose(listed.clients or {}, near_session, preferred)
  if not client then
    error("no target Rex app is connected", 0)
  end
  must(
    action,
    rex.call("client.perform", { client_id = client, action = action, args = args or {} })
  )
end

---Show a session, optionally at one of its windows.
---@param session_id string
---@param window_id? string
---@param near_session? string
---@param preferred? string
function M.show(session_id, window_id, near_session, preferred)
  M.perform(
    "session.select",
    { session_id = session_id, window_id = window_id },
    near_session,
    preferred
  )
end

return M
