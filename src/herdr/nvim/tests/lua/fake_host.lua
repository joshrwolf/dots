-- A domain fake, not a Neovim/global mock. Tests explicitly commit/deliver each
-- publication, including completions arriving after cancellation or a retry.
local M = {}
function M.new(context)
  local fake = {
    publications = {},
    events = {},
    pane = {
      pane = "w1:p1",
      tab = "w1:t1",
      workspace = "w1",
    },
  }
  local host = { server = context.server }
  fake.host = host
  function host.current()
    if fake.unavailable then
      require("dots.herdr.protocol").raise("host_unavailable", "synthetic outage")
    end
    return vim.deepcopy(fake.pane)
  end
  function host.publish(token, done)
    local publication = { delivered = false }
    function publication.commit()
      fake.pane.registration = token
    end
    function publication.deliver(err)
      assert(not publication.delivered, "completion callbacks run once")
      publication.delivered = true
      done(err)
    end
    function publication.cancel()
      table.insert(fake.events, "cancel")
      if publication.on_cancel then
        publication.on_cancel()
      end
    end
    table.insert(fake.publications, publication)
    return publication
  end
  function host.clear(token)
    table.insert(fake.events, "clear")
    if fake.clear_error then
      error("synthetic clear failure")
    end
    if fake.pane.registration == token then
      fake.pane.registration = nil
    end
  end
  function fake.listen()
    return {
      token = context.registration,
      close = function()
        table.insert(fake.events, "close")
      end,
    }
  end
  return fake
end
return M
