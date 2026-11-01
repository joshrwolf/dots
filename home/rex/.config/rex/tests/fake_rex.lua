-- A hand-authored Rex for testing code that reads or changes it, run in Rex's
-- own interpreter. `install` swaps fakes into `rex.call`, `rex.session`,
-- `rex.block.call`, and `rex.exec` through the test's `t.stub`, so the runner
-- restores them. Every call lands in `fake.calls`, in order; any call the fake
-- does not know fails the test.
local process = require("dots.process")
local M = {}

---@class tests.Call
---@field method string Such as `session.attach`, `session.new_layer`, `block.call`, or `exec`.
---@field args table The call's arguments; for `exec`, the argv.

---@class tests.FakeSession
---@field session_id string
---@field label string
---@field view? rex.View Absent when reading the session fails.
---@field terminals? table<string, { process?: table, records?: table[] }>

---@class tests.FakeRex
---@field sessions? tests.FakeSession[]
---@field clients? table[] `client.list` entries; by default one app showing no session.
---@field commands? table<string, table> `rex.exec` results by argv joined with spaces. Unset fields describe a silent success.
---@field errors? table<string, string> Methods that fail, with their error.

---@class tests.InstalledFake: tests.FakeRex
---@field calls tests.Call[]
---@field only fun(...: string): tests.Call[] The calls to these methods, in order.

local TERMINAL = "com.superlogical.terminal"

---@param t tests.T
---@param fake tests.FakeRex
---@return tests.InstalledFake
function M.install(t, fake)
  t.stub(process, "environment", function()
    return { PATH = "/bin" }
  end)
  t.stub(process, "which", function(name)
    return "/bin/" .. name
  end)
  local installed = fake --[[@as tests.InstalledFake]]
  installed.sessions = fake.sessions or {}
  installed.clients = fake.clients or { { client_id = "app", info = { kind = "app" } } }
  installed.commands = fake.commands or {}
  installed.errors = fake.errors or {}
  installed.calls = {}
  installed.only = function(...)
    local wanted, out = {}, {}
    for _, method in ipairs({ ... }) do
      wanted[method] = true
    end
    for _, call in ipairs(installed.calls) do
      if wanted[call.method] then
        out[#out + 1] = call
      end
    end
    return out
  end

  local by_id = {}
  for _, session in ipairs(installed.sessions) do
    by_id[session.session_id] = session
  end

  ---Log a call, then fail it as `errors` says or let `respond` answer.
  ---@param method string
  ---@param args? table
  ---@param respond fun(): (table?, string?)
  ---@return table?
  ---@return string?
  local function answer(method, args, respond)
    installed.calls[#installed.calls + 1] = { method = method, args = args or {} }
    if installed.errors[method] then
      return nil, installed.errors[method]
    end
    return respond()
  end

  local function done()
    return {}
  end

  local methods = {
    ["session.list"] = function()
      local listed = {}
      for _, session in ipairs(installed.sessions) do
        listed[#listed + 1] = { session_id = session.session_id, label = session.label }
      end
      return { sessions = listed }
    end,
    ["session.attach"] = done,
    ["session.create"] = function()
      return { session_id = "created" }
    end,
    ["client.list"] = function()
      return { clients = installed.clients }
    end,
    ["client.perform"] = done,
    ["session.destroy"] = done,
  }
  t.stub(rex, "call", function(method, args)
    local respond = methods[method] or error("unexpected rex.call " .. method, 0)
    return answer(method, args, respond)
  end)

  local helpers = {
    view = function(args)
      local session = by_id[args.session_id]
      if not (session and session.view) then
        return nil, "session not found"
      end
      return session.view
    end,
    new_window = function()
      return { window_id = "link", block_ids = { "link" } }
    end,
    new_layer = function()
      return { layer_id = "layer", block_ids = { "picker" } }
    end,
    close_layer = done,
    zoom_block = done,
    focus_block = done,
    focus_direction = done,
  }
  local session_api = {}
  for name, respond in pairs(helpers) do
    session_api[name] = function(args)
      return answer("session." .. name, args, function()
        return respond(args)
      end)
    end
  end
  t.stub(
    rex,
    "session",
    setmetatable(session_api, {
      __index = function(_, name)
        error("unexpected rex.session." .. tostring(name), 0)
      end,
    })
  )

  t.stub(rex.block, "call", function(creator, method, args)
    local call = { creator = creator, method = method, args = args }
    return answer("block.call", call, function()
      local session = by_id[args.session_id]
      local terminal = session and session.terminals and session.terminals[args.block_id]
      if creator ~= TERMINAL or not terminal then
        return nil, "not found"
      end
      if method == "process" then
        return terminal.process
      elseif method == "program_status" then
        return { records = terminal.records or {} }
      elseif method == "title" then
        return { title = terminal.title or "" }
      end
      error("unexpected terminal method " .. method, 0)
    end)
  end)

  local exec = {
    start = function(argv)
      error("unexpected rex.exec.start " .. table.concat(argv, " "), 0)
    end,
  }
  t.stub(
    rex,
    "exec",
    setmetatable(exec, {
      __call = function(_, argv)
        local command = table.concat(argv, " ")
        local result = installed.commands[command] or error("unexpected command " .. command, 0)
        return answer("exec", argv, function()
          local ran = { code = 0, ok = true, stdout = "", stderr = "", timed_out = false }
          for key, value in pairs(result) do
            ran[key] = value
          end
          if result.ok == false and result.code == nil then
            ran.code = 1
          end
          return ran
        end)
      end,
    })
  )

  return installed
end

---The actions and bindings a feature's `install` registers, keyed by action
---name, with bindings in order.
---@param t tests.T
---@param feature { install: fun() }
---@return table<string, rex.ActionSpec> actions
---@return { key: string, target: string|rex.KeyFn, args?: table }[] bindings
function M.registered(t, feature)
  local actions, bindings = {}, {}
  t.stub(rex, "action", function(spec)
    if actions[spec.name] then
      error("action " .. spec.name .. " is registered twice", 0)
    end
    actions[spec.name] = spec
  end)
  t.stub(rex, "bind", function(key, target, args)
    bindings[#bindings + 1] = { key = key, target = target, args = args }
  end)
  feature.install()
  return actions, bindings
end

return M
