local config = assert(vim.env.HERDR_REVIEW_NVIM_CONFIG)
package.path = config .. "/lua/?.lua;" .. config .. "/lua/?/init.lua;" .. package.path

-- Exercise the real asynchronous client and JSON framing without a live editor,
-- bridge process, or repository. Only the process transport is replaced.
local transport
local requests = {}
local handler
vim.g.herdr_review_binary = "/bin/sh"
vim.fn.jobstart = function(_, options) transport = options; return 1 end
vim.fn.jobstop = function() return 1 end
vim.fn.chanclose = function() return 1 end
vim.fn.chansend = function(_, frame)
  local request = vim.json.decode(frame)
  table.insert(requests, request)
  local result, err
  if request.method == "hello" then
    result = { protocol = 1, capabilities = { "binding.load", "context.load", "thread.create", "binding.observe" } }
  else
    result, err = handler(request)
  end
  vim.schedule(function()
    local response = { id = request.id }
    if err then response.error = err else response.result = result end
    transport.on_stdout(1, { vim.json.encode(response), "" })
  end)
  return #frame
end

local client = require("herdr_review.client")
local function page(cursor, messages, attempts)
  return {
    context = { id = "context" }, binding = { id = "binding" }, observation = { id = "observation" },
    threads = { { id = "thread", messages = messages } },
    agent_requests = { { id = "request", attempts = attempts } },
    next_cursor = cursor,
  }
end
local first = page({ offset = 1 }, { { id = "m1", body = "café 👩‍💻" } }, { { id = "a1" } })
local last = page(nil, { { id = "m2", body = "answer" } }, { { id = "a2" } })
handler = function(request)
  return request.params.cursor and vim.deepcopy(last) or vim.deepcopy(first)
end
local function run(start)
  local done, result, err = false, nil, nil
  start(function(value, failure) result, err, done = value, failure, true end)
  assert(vim.wait(1000, function() return done end), "paged client request timed out")
  return result, err
end
local result, err = run(function(callback) client.load_binding("/tmp/review-pages", "binding", callback) end)
assert(not err and result.next_cursor == nil)
assert(#result.threads == 1 and #result.threads[1].messages == 2)
assert(result.threads[1].messages[1].body == "café 👩‍💻")
assert(#result.agent_requests == 1 and #result.agent_requests[1].attempts == 2)

local cached = vim.deepcopy(result)
cached.revision = 7
handler = function(request)
  assert(request.params.if_revision == 7 and request.params.if_observation_id == "observation")
  return { unchanged = true }
end
result, err = run(function(callback) client.load_binding("/tmp/review-pages", "binding", callback, cached) end)
assert(not err and result == cached, "unchanged reads reuse the workflow-owned state")

handler = function()
  local single = vim.deepcopy(first)
  single.next_cursor = nil
  table.insert(single.threads, vim.deepcopy(last.threads[1]))
  table.insert(single.agent_requests, vim.deepcopy(last.agent_requests[1]))
  return single
end
result, err = run(function(callback) client.load_binding("/tmp/review-pages", "binding", callback) end)
assert(not err and #result.threads == 1 and #result.threads[1].messages == 2,
  "fragments on the first/only page must merge too")
assert(#result.agent_requests == 1 and #result.agent_requests[1].attempts == 2)

local stale = true
handler = function(request)
  if request.params.cursor and stale then
    stale = false
    return nil, { code = "stale_cursor", message = "review changed during pagination" }
  end
  return request.params.cursor and vim.deepcopy(last) or vim.deepcopy(first)
end
result, err = run(function(callback) client.load_binding("/tmp/review-pages", "binding", callback) end)
assert(not err and #result.threads[1].messages == 2, "stale reads restart without duplicating fragments")

local creates = 0
handler = function(request)
  if request.method == "thread.create" then creates = creates + 1; return vim.deepcopy(first) end
  return vim.deepcopy(last)
end
result, err = run(function(callback) client.create_thread("/tmp/review-pages", "binding", {}, {}, callback) end)
assert(not err and creates == 1 and #result.threads[1].messages == 2, "pagination must never repeat writes")

handler = function() return vim.deepcopy(first) end
result, err = run(function(callback) client.load_binding("/tmp/review-pages", "binding", callback) end)
assert(not result and err.code == "invalid_response", "non-advancing cursors must terminate")
handler = function(request)
  if request.params.cursor then return {} end
  return vim.deepcopy(first)
end
result, err = run(function(callback) client.load_binding("/tmp/review-pages", "binding", callback) end)
assert(not result and err.code == "invalid_response", "malformed pages must report errors instead of throwing")
client.stop("/tmp/review-pages")
print("herdr_review pages: ok")
