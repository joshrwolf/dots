-- Attention: only the agents that want the user, then those still working,
-- each with a live preview of its screen. Idle agents stay out of the way.
local agents = require("dots.agents")
local snapshot = require("dots.snapshot")
local views = require("dots.views")

---@class dots.Attention
local M = {}

---@param snap dots.Snapshot
---@return pick.View view
function M.view(snap)
  local attention, others = agents.split(snap)
  local items = {} ---@type dots.AgentRow[]
  for _, entry in ipairs(attention) do
    items[#items + 1] = { group = "attention", entry = entry }
  end
  for _, entry in ipairs(others) do
    if entry.block.status.state == "working" then
      items[#items + 1] = { group = "working", entry = entry }
    end
  end
  return {
    prompt = "attention› ",
    accept = "go_to",
    groups = {
      { id = "attention", label = "needs attention", columns = agents.columns },
      { id = "working", label = "working", columns = agents.columns },
    },
    rows = agents.rows(items, snap),
  }
end

function M.install()
  rex.action({
    name = "attention",
    title = "Agents needing attention…",
    description = "Agents that are blocked, failed, or finished, then those working",
    category = "Navigation",
    keywords = "agents blocked waiting done failed attention inbox",
    run = function(ctx)
      views.open({ title = "attention", view = "attention_view", at = ctx })
    end,
  })
  rex.action({
    name = "attention_view",
    title = "Attention: view",
    description = "The Attention picker's rows, for dots-pick",
    category = "Navigation",
    run = function(ctx)
      local opts = { exclude = ctx.block_id }
      return views.live(M.view(snapshot.collect(opts)), snapshot.events(opts))
    end,
  })
end

return M
