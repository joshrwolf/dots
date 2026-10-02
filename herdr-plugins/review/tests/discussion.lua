-- Real UI/client/store integration, driven by the Rust test's disposable
-- checkout and Herdr socket. Only current-pane discovery and UI choices are
-- supplied by the fixture; JSON framing and the review process are real.
vim.opt.runtimepath:prepend(assert(vim.env.REVIEW_CONFIG))
vim.g.herdr_review_binary = assert(vim.env.REVIEW_BINARY)
local workspace = "w1"
package.loaded["dots.herdr.host"] = {
  new = function()
    return {
      current = function()
        return { workspace = workspace, tab = workspace .. ":t1", pane = workspace .. ":p1" }
      end,
    }
  end,
}
local messages = {}
vim.notify = function(message)
  table.insert(messages, message)
end
vim.ui.select = function(items, _, callback)
  callback(items[1])
end
vim.ui.input = function(_, callback)
  callback("Reviewed café 👩‍💻")
end
local review = require("dots.review")
review.setup()
local function contains(text)
  return table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n"):find(text, 1, true) ~= nil
end
local function wait_for(text)
  assert(
    vim.wait(10000, function()
      return contains(text)
    end, 10),
    text .. ": " .. table.concat(messages, "\n")
  )
end
vim.cmd.ReviewThreads()
wait_for("Explain this change")
assert(contains("example.txt:1-1"))
if vim.env.REVIEW_PHASE == "write" then
  vim.cmd.ReviewReply()
  wait_for("Reviewed café 👩‍💻")
  -- Moving the editor cannot leave its old stdio process authoritative. A
  -- mutation against the old binding must fail in the real repository bridge.
  workspace = "w2"
  local notices = #messages
  vim.ui.input = function(_, callback)
    callback("Must not be saved in another workspace")
  end
  vim.cmd.ReviewReply()
  assert(
    vim.wait(10000, function()
      return #messages > notices or contains("Must not be saved in another workspace")
    end, 10),
    "moved caller was not rejected"
  )
  assert(not contains("Must not be saved in another workspace"), "moved caller mutated the old binding")
  workspace = "w1"
  local buf = vim.api.nvim_get_current_buf()
  local tick = vim.api.nvim_buf_get_changedtick(buf)
  vim.cmd.ReviewRefresh()
  assert(
    vim.wait(10000, function()
      return vim.api.nvim_buf_get_changedtick(buf) > tick
    end, 10),
    "refresh did not finish"
  )
  vim.cmd.ReviewResolve()
  wait_for("# Review thread · resolved")
else
  assert(contains("Reviewed café 👩‍💻"))
  assert(contains("# Review thread · resolved"))
end
-- Closing the editor owns only the stdio connection, not the review data.
require("dots.review.client").stop(vim.fn.getcwd())
vim.cmd("qa!")
