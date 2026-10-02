-- An unadvertised headless editor fixture. Real endpoint/protocol/dispatcher,
-- no pretend-successful metadata publication to manufacture local readiness.
local tests = assert(vim.env.HERDR_NVIM_TESTS)
dofile(tests .. "/lua/child.lua")
local listener =
  require("dots.herdr.listener").open(vim.env.HERDR_NVIM_SCRATCH .. "/s", vim.env.HERDR_NVIM_RESERVED_REGISTRATION)
local context = require("support").context()
context.registration = listener.token
local Endpoint = require("dots.herdr.endpoint")
local Bridge = require("dots.herdr.bridge")
local ownership, owner_clock = nil, 0
local endpoint = Endpoint.new(context.server, listener)
local dispatch = Bridge.new(endpoint)
local received, last_reply = 0, nil
package.loaded["dots.herdr"] = {
  request = function(json)
    received = received + 1
    last_reply = dispatch(json)
    return last_reply
  end,
}
local finished = false
_G.fixture = {
  received = function()
    return received
  end,
  last_reply = function()
    return last_reply
  end,
  snapshot = function()
    return vim.json.encode({
      path = vim.api.nvim_buf_get_name(0),
      cursor = vim.api.nvim_win_get_cursor(0),
      buffers = vim.api.nvim_list_bufs(),
    })
  end,
  manage = function(owner)
    assert(received == 0, "cannot change management mode after use")
    ownership = require("dots.herdr.ownership").new(owner, function()
      return owner_clock
    end)
    endpoint = Endpoint.new(context.server, listener, ownership)
    dispatch = Bridge.new(endpoint)
    return true
  end,
  advance_owner_clock = function(milliseconds)
    local amount = assert(tonumber(milliseconds))
    assert(amount >= 0)
    owner_clock = owner_clock + amount
    return true
  end,
  configure = function(server)
    -- Test bootstrap only: bind the fixture to the fake Herdr server after its
    -- script is constructed using our random token. Never rebind after use.
    assert(received == 0, "cannot reconfigure an active fixture")
    context.server = server
    endpoint = Endpoint.new(server, listener, ownership)
    dispatch = Bridge.new(endpoint)
    return true
  end,
  stop = function()
    finished = true
    return true
  end,
  fail_presentation = function(path)
    vim.fn.bufload(vim.fn.bufadd(path))
    vim.api.nvim_create_autocmd("BufEnter", {
      once = true,
      callback = function()
        error("synthetic BufEnter failure")
      end,
    })
    return true
  end,
}
vim.api.nvim_create_autocmd("VimLeavePre", {
  callback = function()
    endpoint.close()
  end,
})
vim.fn.writefile(
  { vim.json.encode({ socket = listener.socket, context = context, pid = vim.fn.getpid() }) },
  assert(vim.env.HERDR_NVIM_READY)
)
if not vim.wait(30000, function()
  return finished
end, 10) then
  vim.cmd("cquit 1")
end
vim.cmd("qa!")
