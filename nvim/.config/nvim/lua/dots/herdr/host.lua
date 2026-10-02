-- The only Lua module that knows Herdr CLI/wire shapes. Lifecycle and editor
-- tests substitute this small adapter, not vim.system or the Neovim API.
local P = require("dots.herdr.protocol")
local M = {}
local timeout = 1500
local source, token_name = "dots-nvim", "nvim_editor"

---@class dots.herdr.Pane
---@field pane string
---@field tab string
---@field workspace string
---@field registration? string
---@class dots.herdr.Pending
---@field cancel fun() Settle started publication before a subsequent clear.
---@class dots.herdr.Host
---@field server string
---@field current fun(): dots.herdr.Pane
---@field publish fun(token: string, done: fun(error: dots.herdr.Failure?)): dots.herdr.Pending
---@field clear fun(token: string)

local function result_pane(result)
  P.check(result.code == 0, "host_unavailable", result.stderr or "Herdr command failed")
  local ok, response = pcall(vim.json.decode, result.stdout)
  P.check(
    ok and type(response) == "table" and type(response.result) == "table" and response.result.type == "pane_current",
    "host_unavailable",
    "invalid Herdr pane response"
  )
  local pane = response.result.pane
  P.check(
    type(pane) == "table"
      and type(pane.pane_id) == "string"
      and type(pane.tab_id) == "string"
      and type(pane.workspace_id) == "string",
    "host_unavailable",
    "missing Herdr pane identity"
  )
  return {
    pane = pane.pane_id,
    tab = pane.tab_id,
    workspace = pane.workspace_id,
    registration = pane.tokens and pane.tokens[token_name],
  }
end

---@param server string
---@param execute? function Narrow process seam for adapter tests only.
---@return dots.herdr.Host
function M.new(server, execute)
  execute = execute or vim.system
  local host = { server = server }
  local function run(args, callback)
    P.check(vim.env.HERDR_SOCKET_PATH == server, "host_unavailable", "Herdr server environment changed")
    return execute(args, { text = true, timeout = timeout }, callback)
  end
  function host.current()
    local ok, result = pcall(function()
      return result_pane(run({ "herdr", "pane", "current", "--current" }):wait(timeout))
    end)
    if not ok then
      P.raise("host_unavailable", tostring(result))
    end
    return result
  end
  function host.publish(token, done)
    local active, job = true, nil
    local function complete(err)
      job = nil
      if active then
        active = false
        done(err)
      end
    end
    job = run({ "herdr", "pane", "current", "--current" }, function(result)
      vim.schedule(function()
        if not active then
          return
        end
        job = nil
        local ok, err = pcall(function()
          local pane = result_pane(result)
          job = run(
            { "herdr", "pane", "report-metadata", pane.pane, "--source", source, "--token", token_name .. "=" .. token },
            function(report)
              vim.schedule(function()
                if report.code == 0 then
                  complete(nil)
                else
                  complete({ code = "host_unavailable", message = report.stderr or "registration failed" })
                end
              end)
            end
          )
        end)
        if not ok then
          complete(P.failure(err, "host_unavailable"))
        end
      end)
    end)
    return {
      cancel = function()
        -- A queued lookup callback may not start publication after shutdown. An
        -- already-running report is settled before the caller clears metadata.
        active = false
        if job then
          job:wait(timeout)
          job = nil
        end
      end,
    }
  end
  function host.clear(token)
    local pane = host.current()
    if pane.registration ~= token then
      return
    end
    local result =
      run({ "herdr", "pane", "report-metadata", pane.pane, "--source", source, "--clear-token", token_name }):wait(
        timeout
      )
    P.check(result.code == 0, "host_unavailable", result.stderr or "could not clear registration")
  end
  return host
end

return M
