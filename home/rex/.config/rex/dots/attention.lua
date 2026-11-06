-- The global human inbox. Rust owns requests, answers and presentation;
-- Rex owns conversation discovery, navigation and the popup's lifetime.
local hosts = require("dots.hosts")
local jumps = require("dots.jumps")
local snapshot = require("dots.snapshot")
local sessions = require("dots.sessions")
local status = require("dots.status")
local process = require("dots.process")
local views = require("dots.views")

local M = {}

---The viewer across every registered project.
---@param dots string The dots command.
---@return string[]
function M.command(dots)
  return { dots, "attention", "view", "--scope", "global" }
end

---Exact reported runtime owners; multiple terminals never pick an arbitrary one.
---@param snap dots.Snapshot
---@return table<string, table>
function M.conversations(snap)
  local owners = {}
  for _, session in ipairs(snap.sessions) do
    for _, block in ipairs(session.blocks) do
      local owner = block.agent_owner
      if owner then
        if owners[owner] then
          owners[owner] = { state = "ambiguous" }
        else
          owners[owner] = {
            state = "reported",
            label = session.label,
            session = session.id,
            block = block.id,
          }
        end
      end
    end
  end
  return owners
end

function M.install()
  rex.action({
    name = "attention_sessions",
    title = "Attention: sessions",
    description = "Read exact runtime-to-conversation associations from native program status",
    run = function(ctx)
      local forwarded, result = hosts.forward(ctx, "attention_sessions", {})
      if forwarded then
        return result
      end
      return { owners = M.conversations(snapshot.collect({ exclude = ctx.block_id })) }
    end,
  })
  rex.action({
    name = "attention_open",
    title = "Attention: open conversation",
    description = "Focus the exact reporting runtime, without starting an agent",
    args = { owner = "string", session = "string", block = "string" },
    run = function(ctx, args)
      local forwarded, result = hosts.forward(ctx, "attention_open", args)
      if forwarded then
        return result
      end
      local owners = M.conversations(snapshot.collect({ exclude = ctx.block_id }))
      local target = owners[args.owner]
      if not target or target.state ~= "reported" then
        error(
          target and "This runtime is reported by multiple terminals"
            or "This runtime is not reported by a Rex terminal",
          0
        )
      end
      if target.session ~= args.session or target.block ~= args.block then
        error("The conversation moved; refresh before opening it", 0)
      end
      local proc = sessions.terminal(target.session, target.block, "process")
      if not proc or not proc.foreground then
        error("Cannot identify the conversation's current terminal process", 0)
      end
      local reported = sessions.terminal(target.session, target.block, "program_status")
      if not reported then
        error("Cannot read the conversation's current program status", 0)
      end
      if status.agent_owner(reported.records or {}, proc.foreground.pid) ~= args.owner then
        error("The selected terminal does not report this runtime; refresh before opening it", 0)
      end
      jumps.open_block(views.context(ctx), target.session, target.block)
    end,
  })
  rex.action({
    name = "attention",
    title = "Attention…",
    description = "Questions and actions that need you, organized by session across projects",
    category = "Navigation",
    keywords = "attention inbox requests questions decisions answer goals tasks work",
    run = function(ctx)
      local forwarded, result = hosts.forward(ctx, "attention", {})
      if forwarded then
        return result
      end
      local dots = process.which("dots") or error("dots is not installed on the command path", 0)
      views.open({ title = "attention", command = M.command(dots), at = ctx })
    end,
  })
  rex.bind("cmd+i", "attention")
end

return M
