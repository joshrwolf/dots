-- Resolve the invoking editor's current pane, not stale inherited pane IDs.
local M = {}
local timeout = 1500

function M.new(server, execute)
  execute = execute or vim.system
  return {
    current = function()
      assert(vim.env.HERDR_SOCKET_PATH == server, "Herdr server environment changed")
      local result = execute({ "herdr", "pane", "current", "--current" }, {
        text = true,
        timeout = timeout,
      }):wait(timeout)
      assert(result.code == 0, result.stderr or "Herdr command failed")
      local response = vim.json.decode(result.stdout)
      assert(
        type(response) == "table" and type(response.result) == "table"
          and response.result.type == "pane_current",
        "invalid Herdr pane response"
      )
      local pane = response.result.pane
      assert(type(pane) == "table", "missing Herdr pane identity")
      for _, field in ipairs({ "pane_id", "tab_id", "workspace_id" }) do
        assert(type(pane[field]) == "string" and pane[field] ~= "", "missing Herdr pane identity")
      end
      return { pane = pane.pane_id, tab = pane.tab_id, workspace = pane.workspace_id }
    end,
  }
end

return M
