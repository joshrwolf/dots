-- Runs every tests/*_test.lua in Rex's own interpreter, with Rex API calls
-- refused by tests/isolate.lua unless a test fakes them:
--   rex --autostart=false do tests/run.lua [--args '{"only":"go › go_to"}']
-- A suite returns a table of test functions keyed by name. Each test gets a
-- `tests.T`; whatever it stubs is restored when it ends, pass or fail.
local source = debug.getinfo(1, "S").source:gsub("^@", "")
local root = source:match("^(.*)/tests/run%.lua$") or "."
package.path = root .. "/?.lua;" .. root .. "/?/init.lua;" .. package.path

require("tests.isolate").install()

---What a test may change for its own duration.
---@class tests.T
---@field stub fun(tbl: table, key: any, value: any) Set `tbl[key]` until the test ends.

---@param text string
---@param from string
---@param to string
---@return string
local function replace(text, from, to)
  local out, at = {}, 1
  while true do
    local i, j = text:find(from, at, true)
    if not i then
      break
    end
    out[#out + 1] = text:sub(at, i - 1) .. to
    at = j + 1
  end
  out[#out + 1] = text:sub(at)
  return table.concat(out)
end

---@param path string
---@return string
local function relative(path)
  return replace(path, root .. "/", "")
end

---The traceback frame to blame: the innermost line of a test file, else the
---innermost frame outside the runner.
---@param trace string
---@return string?
local function location(trace)
  local fallback
  for frame in trace:gmatch("\n%s*([^\n]-:%d+):") do
    if frame:find("_test%.lua:%d+$") then
      return relative(frame)
    end
    if not fallback and not frame:find("^%[G%]") and not frame:find("/tests/run%.lua:") then
      fallback = relative(frame)
    end
  end
  return fallback
end

---@return string[]
local function suites()
  local listed = rex.exec({ "/bin/ls", root .. "/tests" })
  if not (listed and listed.ok) then
    error("listing tests: " .. (listed and listed.stderr or "rex.exec failed"), 0)
  end
  local names = {}
  for file in listed.stdout:gmatch("[^\n]+") do
    local name = file:match("^(.+)_test%.lua$")
    if name then
      names[#names + 1] = name
    end
  end
  table.sort(names)
  return names
end

---Run `fn` with a fresh `tests.T`, then undo its stubs, newest first.
---@param fn fun(t: tests.T)
---@return string? failure
local function run(fn)
  local stubs = {}
  local t = {
    stub = function(tbl, key, value)
      stubs[#stubs + 1] = { tbl = tbl, key = key, value = tbl[key] }
      tbl[key] = value
    end,
  }
  local message, trace
  xpcall(function()
    fn(t)
  end, function(err)
    message, trace = tostring(err), debug.traceback("", 2)
  end)
  for i = #stubs, 1, -1 do
    stubs[i].tbl[stubs[i].key] = stubs[i].value
  end
  if not message then
    return nil
  end
  local at, text = location(trace), relative(message)
  if at and text:sub(1, #at + 1) ~= at .. ":" then
    return at .. ": " .. text
  end
  return text
end

local only = type(rex.args) == "table" and rex.args.only or nil
local failures, count, skipped = {}, 0, 0
for _, suite in ipairs(suites()) do
  local loaded, tests = pcall(require, "tests." .. suite .. "_test")
  if not loaded or type(tests) ~= "table" then
    count = count + 1
    failures[#failures + 1] = suite .. "\n    " .. relative(tostring(tests))
  else
    local names = {}
    for name in pairs(tests) do
      names[#names + 1] = name
    end
    table.sort(names)
    for _, name in ipairs(names) do
      local full = suite .. " › " .. name
      if only and not full:find(only, 1, true) then
        skipped = skipped + 1
      else
        count = count + 1
        local failure = run(tests[name])
        if failure then
          failures[#failures + 1] = full .. "\n    " .. failure:gsub("\n", "\n    ")
        end
      end
    end
  end
end

local skip_note = skipped > 0 and string.format(", %d skipped", skipped) or ""
if #failures > 0 then
  print(table.concat(failures, "\n"))
  print(string.format("%d of %d tests failed%s", #failures, count, skip_note))
  rex.stop(1)
end
print(string.format("%d tests passed%s", count, skip_note))
