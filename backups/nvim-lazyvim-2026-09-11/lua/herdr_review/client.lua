local M = {}

local PROTOCOL_VERSION = 1
local MAX_FRAME_BYTES = 1024 * 1024
local DEFAULT_REQUEST_TIMEOUT_MS = 10000
local REQUEST_TIMEOUT_MS = {
  ["binding.observe"] = 130000,
  ["thread.create"] = 130000,
}
local MAX_STDERR_BYTES = 64 * 1024
local MAX_STDERR_LINES = 200

local clients = {}
local next_request_id = 0

local function bridge_error(code, message)
  return { code = code, message = message }
end

local function callback_later(callback, result, err)
  vim.schedule(function()
    callback(result, err)
  end)
end

local function staged_binary()
  local config = vim.uv.fs_realpath(vim.fn.stdpath("config"))
  if not config then
    return nil
  end
  local package = vim.fs.dirname(vim.fs.dirname(config))
  local dots = vim.fs.dirname(package)
  local candidate = vim.fs.joinpath(dots, "herdr-plugins", "review", "bin", "herdr-review")
  return vim.fn.executable(candidate) == 1 and candidate or nil
end

local function binary()
  if vim.g.herdr_review_binary then
    if vim.fn.executable(vim.g.herdr_review_binary) == 1 then
      return vim.g.herdr_review_binary
    end
    return nil, "g:herdr_review_binary is not executable: " .. vim.g.herdr_review_binary
  end
  local staged = staged_binary()
  if staged then
    return staged
  end
  local installed = vim.fn.exepath("herdr-review")
  if installed ~= "" then
    return installed
  end
  return nil, "herdr-review is not built; run make plugin-build-review"
end

local function finish(client, err)
  if clients[client.checkout_root] == client then
    clients[client.checkout_root] = nil
  end
  client.state = "stopped"
  for id, callback in pairs(client.pending) do
    client.pending[id] = nil
    callback_later(callback, nil, err)
  end
  for _, callback in ipairs(client.waiting) do
    callback_later(callback, nil, err)
  end
  client.waiting = {}
end

local function poison(client, err)
  finish(client, err)
  if client.job then
    vim.fn.jobstop(client.job)
  end
end

local function valid_response_id(id)
  return type(id) == "number" and id >= 1 and id == math.floor(id)
end

local function accept(client, line)
  if line == "" then
    return
  end
  local ok, response = pcall(vim.json.decode, line)
  if not ok or type(response) ~= "table" or not valid_response_id(response.id) then
    poison(client, bridge_error("invalid_response", "review bridge returned an invalid response frame"))
    return
  end
  local callback = client.pending[response.id]
  if not callback then
    poison(client, bridge_error("invalid_response", "review bridge returned an unexpected response id"))
    return
  end
  local has_result = response.result ~= nil
  local has_error = response.error ~= nil
  if has_result == has_error then
    poison(client, bridge_error("invalid_response", "review bridge response must contain exactly one of result or error"))
    return
  end
  if has_error then
    local err = response.error
    if type(err) ~= "table" or type(err.code) ~= "string" or type(err.message) ~= "string" then
      poison(client, bridge_error("invalid_response", "review bridge returned an invalid error"))
      return
    end
    client.pending[response.id] = nil
    callback_later(callback, nil, err)
    return
  end
  client.pending[response.id] = nil
  local result = response.result
  if result == vim.NIL then
    result = nil
  end
  callback_later(callback, result, nil)
end

local function accept_stdout(client, data)
  if not data or #data == 0 then
    return
  end
  data[1] = client.partial .. data[1]
  client.partial = table.remove(data) or ""
  if #client.partial > MAX_FRAME_BYTES then
    finish(client, bridge_error("response_too_large", "review bridge response exceeds 1 MiB"))
    vim.fn.jobstop(client.job)
    return
  end
  for _, line in ipairs(data) do
    if #line > MAX_FRAME_BYTES then
      finish(client, bridge_error("response_too_large", "review bridge response exceeds 1 MiB"))
      vim.fn.jobstop(client.job)
      return
    end
    accept(client, line)
  end
end

local function send(client, method, params, callback)
  next_request_id = next_request_id + 1
  local id = next_request_id
  local ok, frame = pcall(vim.json.encode, { id = id, method = method, params = params })
  if not ok then
    callback_later(callback, nil, bridge_error("encode_failed", "could not encode request: " .. frame))
    return
  end
  if #frame > MAX_FRAME_BYTES then
    callback_later(callback, nil, bridge_error("request_too_large", "review bridge request exceeds 1 MiB"))
    return
  end

  client.pending[id] = callback
  local sent, result = pcall(vim.fn.chansend, client.job, frame .. "\n")
  if not sent or result == 0 then
    local message = sent and "review bridge stdin is closed" or ("could not write request: " .. result)
    poison(client, bridge_error("send_failed", message))
    return
  end
  local timeout = REQUEST_TIMEOUT_MS[method] or DEFAULT_REQUEST_TIMEOUT_MS
  vim.defer_fn(function()
    if client.pending[id] then
      finish(client, bridge_error("timeout", "request timed out after " .. timeout .. "ms; reconnecting before the next operation"))
      vim.fn.jobstop(client.job)
    end
  end, timeout)
end

local function validate_hello(result)
  if type(result) ~= "table" or result.protocol ~= PROTOCOL_VERSION or type(result.capabilities) ~= "table" then
    return nil, bridge_error("incompatible_protocol", "herdr-review does not speak review protocol v" .. PROTOCOL_VERSION)
  end
  local capabilities = {}
  for _, capability in ipairs(result.capabilities) do
    if type(capability) == "string" then
      capabilities[capability] = true
    end
  end
  return capabilities, nil
end

local function start(checkout_root)
  local executable, executable_err = binary()
  if not executable then
    return nil, bridge_error("binary_not_found", executable_err)
  end
  local client = {
    checkout_root = checkout_root,
    job = nil,
    partial = "",
    stderr = {},
    stderr_bytes = 0,
    stderr_truncated = false,
    pending = {},
    waiting = {},
    capabilities = {},
    state = "starting",
  }
  local job = vim.fn.jobstart({ executable, "--stdio", checkout_root }, {
    stdin = "pipe",
    stdout_buffered = false,
    stderr_buffered = false,
    on_stdout = function(_, data)
      accept_stdout(client, data)
    end,
    on_stderr = function(_, data)
      for _, line in ipairs(data or {}) do
        if line ~= "" then
          table.insert(client.stderr, line)
          client.stderr_bytes = client.stderr_bytes + #line + 1
          while #client.stderr > MAX_STDERR_LINES or client.stderr_bytes > MAX_STDERR_BYTES do
            local removed = table.remove(client.stderr, 1)
            client.stderr_bytes = client.stderr_bytes - #removed - 1
            client.stderr_truncated = true
          end
        end
      end
    end,
    on_exit = function(_, code)
      local detail = table.concat(client.stderr, "\n")
      if client.stderr_truncated then
        detail = "[earlier bridge stderr truncated]\n" .. detail
      end
      local message = "review bridge exited with status " .. code
      if detail ~= "" then
        message = message .. ": " .. detail
      end
      finish(client, bridge_error("bridge_exited", message))
    end,
  })
  if job <= 0 then
    return nil, bridge_error("start_failed", "could not start herdr-review (jobstart returned " .. job .. ")")
  end
  client.job = job
  clients[checkout_root] = client
  send(client, "hello", { protocol = PROTOCOL_VERSION }, function(result, err)
    if err then
      finish(client, err)
      vim.fn.jobstop(client.job)
      return
    end
    local capabilities, validation_err = validate_hello(result)
    if not capabilities then
      finish(client, validation_err)
      vim.fn.jobstop(client.job)
      return
    end
    client.capabilities = capabilities
    client.state = "ready"
    local waiting = client.waiting
    client.waiting = {}
    for _, callback in ipairs(waiting) do
      callback_later(callback, client, nil)
    end
  end)
  return client, nil
end

local function with_client(checkout_root, callback)
  checkout_root = vim.fs.normalize(vim.fn.fnamemodify(checkout_root, ":p"))
  local client = clients[checkout_root]
  if not client then
    local err
    client, err = start(checkout_root)
    if not client then
      callback_later(callback, nil, err)
      return
    end
  end
  if client.state == "ready" then
    callback_later(callback, client, nil)
  else
    table.insert(client.waiting, callback)
  end
end

local function invoke(checkout_root, method, params, callback)
  if type(callback) ~= "function" then
    error("review client callback must be a function")
  end
  with_client(checkout_root, function(client, err)
    if not client then
      callback(nil, err)
      return
    end
    if not client.capabilities[method] then
      callback(nil, bridge_error("unsupported_operation", "review bridge does not support " .. method))
      return
    end
    send(client, method, params, callback)
  end)
end

-- A page is bounded on the wire; consumers only see a complete state. Repeated
-- thread/request records carry the next messages/attempts, never replacements.
local function collect_state(checkout_root, initial, callback)
  if not initial then
    callback(nil, nil)
    return
  end
  local function valid_page(page)
    local function records_valid(records, field)
      if type(records) ~= "table" then return false end
      for _, record in ipairs(records) do
        if type(record) ~= "table" or type(record.id) ~= "string" or type(record[field]) ~= "table" then return false end
        for _, value in ipairs(record[field]) do
          if type(value) ~= "table" or type(value.id) ~= "string" then return false end
        end
      end
      return true
    end
    return type(page) == "table" and type(page.binding) == "table" and type(page.binding.id) == "string"
      and type(page.context) == "table" and type(page.context.id) == "string"
      and type(page.observation) == "table" and type(page.observation.id) == "string"
      and records_valid(page.threads, "messages") and records_valid(page.agent_requests, "attempts")
  end
  if not valid_page(initial) then
    callback(nil, bridge_error("invalid_response", "review bridge returned an invalid state page"))
    return
  end
  local state = initial
  local seen = {}
  local restarts = 0
  local function merge(records, fragments, field)
    local by_id = {}
    for _, record in ipairs(records) do by_id[record.id] = record end
    for _, fragment in ipairs(fragments) do
      local record = by_id[fragment.id]
      if record then
        local ids = {}
        for _, value in ipairs(record[field]) do ids[value.id] = true end
        for _, value in ipairs(fragment[field] or {}) do
          if ids[value.id] then return false end
          table.insert(record[field], value)
          ids[value.id] = true
        end
      else
        table.insert(records, fragment)
        by_id[fragment.id] = fragment
      end
    end
    return true
  end
  local function normalize(page)
    local threads, requests = {}, {}
    if not merge(threads, page.threads, "messages") or not merge(requests, page.agent_requests, "attempts") then return false end
    page.threads, page.agent_requests = threads, requests
    return true
  end
  if not normalize(state) then
    callback(nil, bridge_error("invalid_response", "review page repeated a message or attempt"))
    return
  end
  local next_page
  next_page = function()
    local cursor = state.next_cursor
    if not cursor or cursor == vim.NIL then
      state.next_cursor = nil
      callback(state, nil)
      return
    end
    local cursor_key = vim.json.encode(cursor)
    if seen[cursor_key] then
      callback(nil, bridge_error("invalid_response", "review pagination did not advance"))
      return
    end
    seen[cursor_key] = true
    invoke(checkout_root, "binding.load", { binding_id = state.binding.id, cursor = cursor }, function(page, err)
      if err then
        if err.code == "stale_cursor" and restarts < 2 then
          restarts = restarts + 1
          invoke(checkout_root, "binding.load", { binding_id = state.binding.id }, function(fresh, load_err)
            if load_err then callback(nil, load_err); return end
            if not valid_page(fresh) or not normalize(fresh) then
              callback(nil, bridge_error("invalid_response", "review bridge returned an invalid state page")); return
            end
            state, seen = fresh, {}
            next_page()
          end)
        else
          callback(nil, err)
        end
        return
      end
      if not valid_page(page) or page.binding.id ~= state.binding.id
        or page.observation.id ~= state.observation.id or page.context.id ~= state.context.id then
        callback(nil, bridge_error("invalid_response", "review page belongs to a different binding or observation"))
        return
      end
      if not merge(state.threads, page.threads, "messages")
        or not merge(state.agent_requests, page.agent_requests, "attempts") then
        callback(nil, bridge_error("invalid_response", "review page repeated a message or attempt"))
        return
      end
      state.next_cursor = page.next_cursor
      next_page()
    end)
  end
  next_page()
end

local function state_result(checkout_root, callback)
  return function(result, err)
    if err then callback(nil, err); return end
    collect_state(checkout_root, result, callback)
  end
end

function M.load_binding(checkout_root, binding_id, callback, known_state)
  local params = { binding_id = binding_id }
  if known_state and known_state.binding.id == binding_id and known_state.revision then
    params.if_revision = known_state.revision
    params.if_observation_id = known_state.observation.id
  end
  invoke(checkout_root, "binding.load", params, function(result, err)
    if not err and type(result) == "table" and result.unchanged == true then
      if not params.if_revision then
        callback(nil, bridge_error("invalid_response", "unexpected unchanged review response"))
      else
        callback(known_state, nil)
      end
      return
    end
    state_result(checkout_root, callback)(result, err)
  end)
end

function M.load_context(checkout_root, callback)
  invoke(checkout_root, "context.load", vim.empty_dict(), function(result, err)
    if err or not result then callback(result, err); return end
    collect_state(checkout_root, result.state, function(state, load_err)
      if load_err then callback(nil, load_err); return end
      result.state = state
      callback(result, nil)
    end)
  end)
end

function M.observe_binding(checkout_root, binding_id, comparison, callback)
  invoke(checkout_root, "binding.observe", {
    binding_id = binding_id,
    comparison = comparison,
  }, state_result(checkout_root, callback))
end

function M.create_thread(checkout_root, binding_id, comparison, thread, callback)
  invoke(checkout_root, "thread.create", {
    binding_id = binding_id,
    comparison = comparison,
    thread = thread,
  }, state_result(checkout_root, callback))
end

function M.add_message(checkout_root, binding_id, thread_id, message, callback)
  invoke(checkout_root, "thread.add_message", {
    binding_id = binding_id,
    thread_id = thread_id,
    message = message,
  }, callback)
end

function M.resolve_thread(checkout_root, binding_id, thread_id, callback)
  invoke(checkout_root, "thread.resolve", { binding_id = binding_id, thread_id = thread_id }, callback)
end

function M.list_agents(checkout_root, binding_id, callback)
  invoke(checkout_root, "agents.list", { binding_id = binding_id }, callback)
end

function M.create_agent_request(checkout_root, binding_id, thread_ids, assignment, callback)
  invoke(checkout_root, "agent_request.create", {
    binding_id = binding_id,
    thread_ids = thread_ids,
    assignment = assignment,
  }, callback)
end

function M.retry_dispatch(checkout_root, binding_id, request_id, assignment, callback)
  invoke(checkout_root, "dispatch.retry", {
    binding_id = binding_id,
    request_id = request_id,
    assignment = assignment,
  }, callback)
end

function M.stop(checkout_root)
  checkout_root = vim.fs.normalize(vim.fn.fnamemodify(checkout_root, ":p"))
  local client = clients[checkout_root]
  if not client then
    return
  end
  clients[checkout_root] = nil
  vim.fn.chanclose(client.job, "stdin")
end

vim.api.nvim_create_autocmd("VimLeavePre", {
  callback = function()
    for checkout_root in pairs(clients) do
      M.stop(checkout_root)
    end
  end,
})

return M
