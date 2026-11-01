local attention = require("dots.attention")
local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local go = require("dots.go")
local prs = require("dots.prs")
local snapshots = require("tests.fixtures.snapshots")
local watch = require("tests.fixtures.watch")

local root = debug.getinfo(1, "S").source:match("^@?(.*)/tests/features_test%.lua$") or "."

---Fail naming `name` unless it is a registered action.
---@param actions table<string, rex.ActionSpec>
---@param name string
local function registered(actions, name)
  expect.equal({ name, actions[name] ~= nil }, { name, true })
end

---Every feature `init.lua` installs, registered together.
---@param t tests.T
---@return table<string, rex.ActionSpec> actions
---@return { key: string, target: string|rex.KeyFn }[] bindings
local function install_all(t)
  local file = assert(io.open(root .. "/init.lua", "r"))
  local source = file:read("*a")
  file:close()
  local actions, bindings = {}, {}
  for name in source:gmatch('require%("([%w_.]+)"%)%.install%(%)') do
    local found, bound = fake_rex.registered(t, require(name))
    for action, spec in pairs(found) do
      if actions[action] then
        error(name .. " registers " .. action .. " again", 0)
      end
      actions[action] = spec
    end
    for _, binding in ipairs(bound) do
      bindings[#bindings + 1] = binding
    end
  end
  return actions, bindings
end

return {
  ["actions have snake_case names, a title, and a function"] = function(t)
    local actions = install_all(t)
    expect.equal(actions.go ~= nil, true)
    for name, spec in pairs(actions) do
      expect.equal({ name, name:find("^[a-z][a-z0-9_]*$") ~= nil }, { name, true })
      expect.equal({ name, type(spec.title), type(spec.run) }, { name, "string", "function" })
    end
  end,

  ["each key is bound once, to Rex's actions or ours"] = function(t)
    local actions, bindings = install_all(t)
    local seen = {}
    for _, binding in ipairs(bindings) do
      expect.equal({ binding.key, seen[binding.key] }, { binding.key, nil })
      seen[binding.key] = true
      local target = binding.target
      -- Rex's own actions have dotted names; ours never do.
      if type(target) == "string" and not target:find(".", 1, true) then
        registered(actions, target)
      end
    end
  end,

  ["views send rows to registered actions"] = function(t)
    local actions = install_all(t)
    local views = {
      go.view(snapshots.busy),
      attention.view(snapshots.busy),
      prs.view(watch.mixed, snapshots.busy, "dots"),
    }
    for _, view in ipairs(views) do
      registered(actions, view.accept)
      for _, shortcut in ipairs(view.shortcuts or {}) do
        registered(actions, shortcut.action)
      end
    end
  end,
}
