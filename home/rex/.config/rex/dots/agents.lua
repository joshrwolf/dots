-- Terminals as views list them, agents and the tabs they run in: one row per
-- piece of work, reading status, session, what it is, then what it says, and
-- previewing the terminal's screen.
local snapshot = require("dots.snapshot")
local status = require("dots.status")
local views = require("dots.views")

local M = {}

---@class dots.AgentEntry
---@field session dots.Session
---@field block dots.Block The terminal the row describes.
---@field tab? boolean The row goes to the block's tab rather than focusing the block.

---@class dots.AgentRow
---@field group string
---@field entry dots.AgentEntry

---Status, session, what, detail. Every group in a view lines up by position,
---so views that list places put the place in the session's column.
---@type pick.Column[]
M.columns = {
  { width = { cells = 1 } },
  { width = { content = 32 } },
  { width = { content = 32 } },
  { width = "fill" },
}

---Agents that want the user, most urgent then most recent first.
---@param snap dots.Snapshot
---@return dots.AgentEntry[]
function M.wanting(snap)
  local wanting = {}
  for _, session in ipairs(snap.sessions) do
    for _, block in ipairs(session.blocks) do
      if block.status and status.needs_attention(block.status.state) then
        wanting[#wanting + 1] = { session = session, block = block }
      end
    end
  end
  table.sort(wanting, function(a, b)
    local ra, rb = status.rank(a.block.status.state), status.rank(b.block.status.state)
    if ra ~= rb then
      return ra < rb
    end
    return (a.block.status.updated_at or "") > (b.block.status.updated_at or "")
  end)
  return wanting
end

---A shell command as zsh reports it, cut to what it runs: leading `cd DIR &&`
---steps and variable assignments dropped, and paths cut to their last part.
---@param command string
---@return string
function M.command(command)
  local rest = command
  while true do
    local after = rest:match("^%s*cd%s+%S+%s*&&%s*(.*)$")
      or rest:match("^%s*cd%s+%S+%s*;%s*(.*)$")
      or rest:match("^%s*[%a_][%w_]*=%S*%s+(.*)$")
    if not after or not after:match("%S") then
      break
    end
    rest = after
  end
  local words = {}
  for word in rest:gmatch("%S+") do
    if word:find("/", 1, true) and not word:find("://", 1, true) then
      word = snapshot.basename(word)
    end
    words[#words + 1] = word
  end
  return table.concat(words, " ")
end

---The mark a program puts before its terminal title, such as Pi's `π` in
---`π - dots`: a leading word with no letters or digits.
---@param title? string
---@return string?
function M.mark(title)
  local first = title and title:match("^%s*(%S+)%s")
  if first and not first:find("[%w]") then
    return first
  end
  return nil
end

---What a status record says: its message, or a shell command shortened.
---Without one it names only the app, by the mark its terminal title carries
---when there is one, unless the row names the app already: the status glyph
---says the state. A shell reports a command with the command's program as
---its app.
---@param record dots.StatusRecord
---@param named? boolean
---@param title? string The terminal's title.
---@return string
function M.message(record, named, title)
  local app = record.effective_app or record.app
  local msg = record.msg
  if not msg or not msg:match("%S") then
    return (app and not named) and (M.mark(title) or app) or ""
  end
  local first = msg:match("^%s*(%S+)")
  if app and snapshot.basename(first) == app then
    return M.command(msg)
  end
  return msg
end

---Whether a directory's name only repeats the session's label, as `dots` in
---a session named for its checkout or `api@abc-12` in `api:ABC-12`.
---@param name string
---@param label string
---@return boolean
local function repeats(name, label)
  local a, b = name:lower(), label:lower()
  return a == b or (a:gsub("@", ":", 1)) == b
end

---@alias dots.Named "title"|"place"|"app"

---What a row's terminal is, first of: a label someone set, its agent's
---title, its directory unless that only repeats the session, and its
---agent's app. Rex's default labels and terminal titles (Pi's is
---`π - <directory>`) say nothing more, and the foreground process (`node`
---for Pi, `zsh` at a prompt) less.
---@param entry dots.AgentEntry
---@param snap dots.Snapshot
---@return string? what
---@return dots.Named? from
local function identity(entry, snap)
  local block = entry.block
  local label
  if entry.tab then
    label = block.tab_label or block.label
  else
    label = block.label or block.tab_label
  end
  local title = label or (block.status and block.status.title)
  if title and title:match("%S") then
    return title, "title"
  end
  if block.cwd then
    local name = snapshot.name(block.cwd, snap)
    if not repeats(name, entry.session.label) then
      return name, "place"
    end
  end
  local app = block.status and (block.status.effective_app or block.status.app)
  if app and app ~= "" then
    return app, "app"
  end
  return nil
end

---What a row's terminal says: its agent's message or state, or the program
---it runs; and where it is, when nothing else on the row says so.
---@param entry dots.AgentEntry
---@param from? dots.Named
---@param snap dots.Snapshot
---@return string
local function detail(entry, from, snap)
  local block = entry.block
  local parts = {}
  if block.status then
    local said = M.message(block.status, from == "app", block.title)
    if said ~= "" then
      parts[1] = said
    end
  elseif block.program then
    parts[1] = block.program
  end
  local cwd = block.cwd
  if from ~= "place" and cwd and not repeats(snapshot.name(cwd, snap), entry.session.label) then
    parts[#parts + 1] = snapshot.location(cwd, snap)
  end
  return table.concat(parts, " · ")
end

---Rows for `items`, in order. Rows of one session that would read alike are
---told apart by their tab's position, as is one with nothing else to say.
---@param items dots.AgentRow[]
---@param snap dots.Snapshot
---@return pick.Row[]
function M.rows(items, snap)
  -- How many tabs of a session the rows would give each name.
  local named, seen, alike = {}, {}, {}
  for i, item in ipairs(items) do
    local what, from = identity(item.entry, snap)
    named[i] = { what = what, from = from }
    if what then
      local key = item.entry.session.id .. "\n" .. what
      local tab = key .. "\n" .. item.entry.block.tab
      if not seen[tab] then
        seen[tab] = true
        alike[key] = (alike[key] or 0) + 1
      end
    end
  end
  local rows = {} ---@type pick.Row[]
  for i, item in ipairs(items) do
    local session, block = item.entry.session, item.entry.block
    local tab = "tab " .. block.tab
    local what = named[i].what
    if not what then
      what = tab
    elseif alike[session.id .. "\n" .. what] > 1 then
      what = what .. " · " .. tab
    end
    local record = block.status
    local glyph, role = "", "muted"
    if record then
      glyph, role = status.glyph(record.state)
    end
    local target = item.entry.tab and { "-s", session.id, "-w", block.window_id }
      or { "-s", session.id, "-b", block.id }
    rows[i] = {
      id = (item.entry.tab and "tab|" .. session.id .. "|" .. block.window_id)
        or ("block|" .. session.id .. "|" .. block.id),
      group = item.group,
      cells = {
        { text = glyph, role = role, kind = "tag" },
        { text = session.label, role = "blue" },
        { text = what },
        { text = detail(item.entry, named[i].from, snap), role = "muted" },
      },
      search = table.concat({
        record and record.state or "",
        record and (record.effective_app or record.app) or "",
        block.program or "",
        block.cwd or "",
      }, " "),
      preview = views.capture(target),
    }
  end
  return rows
end

return M
