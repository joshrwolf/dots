-- Checkout placement: where a GitHub repository's primary checkout lives,
-- from the JSON rule files in `~/.config/rex/checkouts/`. Each file is
-- `{ "version": 1, "rules": { KEY: PATH } }`, where KEY is `owner/name`,
-- `owner/*`, or `*`, and PATH starts with `~/` or `/` and may use `{owner}`
-- and `{name}`. The most specific key wins, wherever it is defined; two files
-- giving one key different paths is an error naming both.
local process = require("dots.process")

local M = {}

-- The rule directory under home.
M.DIR = "/.config/rex/checkouts"

local VERSION = 1

---Where a repository's checkout goes and which rule said so.
---@class dots.Placement
---@field path string Absolute.
---@field key string The rule's key, such as `owner/*`.
---@field file string The file that defines the rule.

---A rule and the file that defines it.
---@class dots.Rule
---@field path string The template, as written.
---@field file string

---Rules by lower-case key.
---@alias dots.Rules table<string, dots.Rule>

---@param key string
---@return boolean
local function valid_key(key)
  if key == "*" then
    return true
  end
  local owner, name = key:match("^([^/]+)/([^/]+)$")
  return owner ~= nil
    and owner:match("^[%w%-]+$") ~= nil
    and (name == "*" or name:match("^[%w%-_.]+$") ~= nil)
end

---@param template string
---@return boolean
local function valid_path(template)
  if not (template:sub(1, 2) == "~/" or template:sub(1, 1) == "/") then
    return false
  end
  for placeholder in template:gmatch("{([^}]*)}") do
    if placeholder ~= "owner" and placeholder ~= "name" then
      return false
    end
  end
  return true
end

---The rules in one file's text.
---@param file string The file's path, for errors.
---@param text string
---@return table<string, string>? rules Templates by lower-case key.
---@return string? err
function M.parse(file, text)
  local decoded, decode_err = rex.json.decode(text)
  if type(decoded) ~= "table" then
    return nil, file .. ": not JSON: " .. tostring(decode_err or "expected an object")
  end
  for field in pairs(decoded) do
    if field ~= "version" and field ~= "rules" then
      return nil, file .. ": unknown field " .. tostring(field) .. "; expected version and rules"
    end
  end
  if decoded.version ~= VERSION then
    return nil, file .. ": version must be " .. VERSION .. ", not " .. tostring(decoded.version)
  end
  if type(decoded.rules) ~= "table" then
    return nil, file .. ": rules must be an object of KEY: PATH"
  end
  local rules, keys = {}, {}
  for key in pairs(decoded.rules) do
    keys[#keys + 1] = key
  end
  table.sort(keys, function(a, b)
    return tostring(a) < tostring(b)
  end)
  for _, key in ipairs(keys) do
    local template = decoded.rules[key]
    if type(key) ~= "string" or not valid_key(key) then
      return nil, file .. ": rule " .. tostring(key) .. " must be owner/name, owner/*, or *"
    end
    if type(template) ~= "string" or not valid_path(template) then
      return nil,
        file
          .. ": rule "
          .. key
          .. " must be a path starting with ~/ or / that uses only {owner} and {name}"
    end
    local lower = key:lower()
    if rules[lower] and rules[lower] ~= template then
      return nil, file .. ": rule " .. key .. " is given twice with different paths"
    end
    rules[lower] = template
  end
  return rules
end

---Merge rule files. A key may appear in several files only with one path.
---@param files { path: string, text: string }[]
---@return dots.Rules? rules
---@return string? err
function M.merge(files)
  local merged = {} ---@type dots.Rules
  for _, file in ipairs(files) do
    local rules, err = M.parse(file.path, file.text)
    if not rules then
      return nil, err
    end
    local keys = {}
    for key in pairs(rules) do
      keys[#keys + 1] = key
    end
    table.sort(keys)
    for _, key in ipairs(keys) do
      local seen = merged[key]
      if seen and seen.path ~= rules[key] then
        return nil,
          "checkout rule "
            .. key
            .. " conflicts: "
            .. seen.file
            .. " says "
            .. seen.path
            .. ", "
            .. file.path
            .. " says "
            .. rules[key]
      end
      merged[key] = seen or { path = rules[key], file = file.path }
    end
  end
  return merged
end

---Where the most specific rule puts `slug`'s checkout: its `owner/name`
---rule, else its owner's, else the default.
---@param rules dots.Rules
---@param slug string GitHub `owner/name`.
---@param home string
---@return dots.Placement?
function M.resolve(rules, slug, home)
  local owner, name = slug:match("^([^/]+)/([^/]+)$")
  if not owner then
    return nil
  end
  for _, key in ipairs({ slug:lower(), owner:lower() .. "/*", "*" }) do
    local rule = rules[key]
    if rule then
      local path = rule.path:gsub("{owner}", owner):gsub("{name}", name):gsub("/+$", "")
      if path:sub(1, 2) == "~/" then
        path = home .. path:sub(2)
      end
      return { path = path, key = key, file = rule.file }
    end
  end
  return nil
end

---The rule files in home's rule directory, in name order; none when it is
---missing.
---@param home string
---@return string[]
function M.files(home)
  local dir = home .. M.DIR
  local listed = process.run({ "ls", "-1", dir }, { timeout = 5 })
  local files = {}
  if listed and listed.ok then
    for name in listed.stdout:gmatch("[^\n]+") do
      if name:match("%.json$") then
        files[#files + 1] = dir .. "/" .. name
      end
    end
  end
  table.sort(files)
  return files
end

---Home's rules, merged from every rule file.
---@param home string
---@param read fun(path: string): string?
---@return dots.Rules? rules
---@return string? err
function M.load(home, read)
  local files = {}
  for _, path in ipairs(M.files(home)) do
    local text = read(path)
    if not text then
      return nil, path .. ": cannot be read"
    end
    files[#files + 1] = { path = path, text = text }
  end
  return M.merge(files)
end

return M
