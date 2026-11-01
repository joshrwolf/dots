-- Jumping between shell prompts in a terminal's scrollback. Works where the
-- shell marks its prompts (OSC 133), as zsh does in Rex.
local M = {}

function M.install()
  rex.bind("cmd+up", "client.prompt.previous")
  rex.bind("cmd+down", "client.prompt.next")
end

return M
