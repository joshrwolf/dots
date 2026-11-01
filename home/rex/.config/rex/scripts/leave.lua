-- Run by a started tab's shell as it exits (`rex do <this file>`): ends the
-- session when that tab is its last, and leaves it open when other tabs
-- remain. See `dots.sessions.leave`.
-- `rex do` resolves modules beside the script; the config's live one level up.
local here = (debug.getinfo(1, "S").source:gsub("^@", "")):match("^(.*)/scripts/[^/]+$")
package.path = here .. "/?.lua;" .. package.path
local sessions = require("dots.sessions")

local session_id = os.getenv("REX_SESSION")
if not session_id then
  error("leave.lua runs in a Rex terminal, which sets REX_SESSION", 0)
end
local _, err = sessions.leave(session_id)
if err then
  error("leaving the session: " .. err, 0)
end
