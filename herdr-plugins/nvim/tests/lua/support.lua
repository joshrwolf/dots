local M = {}

-- Every case starts a fresh editor. No buffer/module/env state crosses cases.
function M.child_set()
  local child = MiniTest.new_child_neovim()
  local set = MiniTest.new_set({
    hooks = {
      pre_case = function()
        child.restart({ "-u", vim.env.HERDR_NVIM_TESTS .. "/lua/child.lua", "-i", "NONE" })
        -- mini.test adds -n; restore normal swap semantics after startup.
        child.cmd("set updatecount& swapfile&")
      end,
      post_case = child.stop,
      post_once = child.stop,
    },
  })
  return set, child
end

-- Real native UI, owned by this test. Keep rendering off the control RPC
-- channel: a Neovim RPC client is not itself a UI renderer.
function M.ui(child)
  local previous = {}
  for _, ui in ipairs(child.api.nvim_list_uis()) do
    previous[ui.chan] = true
  end
  local job = vim.fn.jobstart(
    { vim.v.progpath, "-u", "NONE", "-i", "NONE", "--server", child.job.address, "--remote-ui" },
    { pty = true, env = { TERM = "xterm-256color" } }
  )
  assert(job > 0, "could not start remote UI")
  local function stop()
    if vim.fn.jobwait({ job }, 0)[1] == -1 then
      vim.fn.jobstop(job)
      assert(vim.fn.jobwait({ job }, 3000)[1] ~= -1, "remote UI did not stop")
    end
  end
  MiniTest.finally(stop)
  local channel
  assert(
    vim.wait(2000, function()
      for _, ui in ipairs(child.api.nvim_list_uis()) do
        if not previous[ui.chan] then
          channel = ui.chan
          return true
        end
      end
      return false
    end, 10),
    "remote UI did not attach"
  )
  return { job = job, channel = channel, stop = stop }
end

function M.context()
  return {
    registration = "0123456789abcdef0123456789abcdef",
    server = "/tmp/example-herdr.sock",
  }
end

return M
