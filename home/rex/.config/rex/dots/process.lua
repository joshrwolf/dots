-- Commands use the selected machine environment, independent of the directory
-- an action inspects. Mise owns tool paths; Rex owns execution on this host.
local M = {}

---@return table<string, string>? env
---@return string? err
function M.environment()
  local home = os.getenv("HOME")
  if not home or home == "" then
    return nil, "HOME is required to resolve the machine environment"
  end
  local bootstrap_path = home .. "/.local/bin:" .. (os.getenv("PATH") or "/usr/bin:/bin")
  local result, err = rex.exec({ os.getenv("MISE_BIN") or "mise", "env", "--json" }, {
    cwd = home,
    env = { PATH = bootstrap_path },
    timeout = 10,
  })
  if not result then
    return nil, "resolving the machine environment: " .. tostring(err)
  end
  if not result.ok then
    return nil, "resolving the machine environment: " .. result.stderr
  end
  local env, decode_err = rex.json.decode(result.stdout)
  if type(env) ~= "table" or type(env.PATH) ~= "string" or env.PATH == "" then
    return nil, "mise env did not return an environment with PATH: " .. tostring(decode_err)
  end
  for key, value in pairs(env) do
    if type(key) ~= "string" or type(value) ~= "string" then
      return nil, "mise env returned a non-string environment entry"
    end
  end
  return env
end

---@param opts? rex.ExecOpts
---@param environment? table<string, string> Reuse one resolution within an operation.
---@return rex.ExecOpts? options
---@return string? err
function M.with_path(opts, environment)
  local err
  if not environment then
    environment, err = M.environment()
  end
  if not environment then
    return nil, err
  end
  local env = {}
  for key, value in pairs(environment) do
    env[key] = value
  end
  local merged = {}
  for key, value in pairs(opts or {}) do
    merged[key] = value
  end
  for key, value in pairs(merged.env or {}) do
    env[key] = value
  end
  merged.env = env
  return merged
end

---Run a command to completion, without changing its working directory.
---@param argv string[]
---@param opts? rex.ExecOpts
---@param environment? table<string, string>
---@return rex.ExecResult?
---@return string? err
function M.run(argv, opts, environment)
  local options, err = M.with_path(opts, environment)
  if not options then
    return nil, err
  end
  return rex.exec(argv, options)
end

---The absolute path of `name` in the machine environment, or nil.
---@param name string
---@param exists? fun(path: string): boolean
---@return string?
---@return string? err
function M.which(name, exists)
  if name:find("/", 1, true) then
    return name
  end
  local env, err = M.environment()
  if not env then
    return nil, err
  end
  exists = exists
    or function(path)
      local file = io.open(path, "r")
      if file then
        file:close()
        return true
      end
      return false
    end
  for dir in env.PATH:gmatch("[^:]+") do
    local candidate = dir .. "/" .. name
    if exists(candidate) then
      return candidate
    end
  end
  return nil
end

return M
