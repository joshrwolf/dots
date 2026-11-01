-- PRs: the watch list agents curate with `dots github watch add`, with cached
-- status. Nothing here waits on GitHub: rows and previews come from the local
-- store, and `dots github watch follow` refreshes what is due while the view
-- is open, which reloads the rows when something changed.
local app = require("dots.app")
local go = require("dots.go")
local jumps = require("dots.jumps")
local must = require("dots.result").must
local process = require("dots.process")
local snapshot = require("dots.snapshot")
local views = require("dots.views")

---@class dots.Prs
local M = {}

---One entry of `dots github watch list --json`.
---@class dots.WatchedPr
---@field url string
---@field repository string `owner/repo`.
---@field number integer
---@field title? string Absent until the first refresh.
---@field state "open"|"draft"|"merged"|"closed"|"unknown"
---@field review "approved"|"changes_requested"|"review_required"|"none"|"unknown"
---@field ci "success"|"failure"|"pending"|"none"|"unknown"
---@field head_ref? string
---@field checkout? string Where the agent that added it was working.
---@field added_by? string
---@field rex_session? string The id of the Rex session the work belongs to.
---@field added_at integer
---@field checked_at? integer
---@field error? string The last refresh error for this PR.

---@class dots.WatchList
---@field prs dots.WatchedPr[]
---@field due integer PRs whose next check has come.

local CI = {
  success = { text = "✓ ci", role = "green" },
  failure = { text = "✗ ci", role = "red" },
  pending = { text = "◷ ci", role = "yellow" },
  none = { text = "— ci", role = "muted" },
  unknown = { text = "? ci", role = "muted" },
}

local REVIEW = {
  approved = { text = "approved", role = "green" },
  changes_requested = { text = "changes", role = "red" },
  review_required = { text = "review", role = "yellow" },
  none = { text = "", role = "muted" },
  unknown = { text = "", role = "muted" },
}

---How many merged and closed PRs the view shows, most recently checked first.
---The rest stay watched until the store drops them.
local DONE_LIMIT = 10

local CLOSED = {
  merged = { text = "merged", role = "accent" },
  closed = { text = "closed", role = "muted" },
}

---Which group a PR belongs in: what needs you, what is ready, the rest, done.
---@param pr dots.WatchedPr
---@return "attention"|"ready"|"open"|"done"
function M.group(pr)
  if pr.state == "merged" or pr.state == "closed" then
    return "done"
  elseif pr.error or pr.ci == "failure" or pr.review == "changes_requested" then
    return "attention"
  elseif pr.state == "open" and pr.review == "approved" and pr.ci == "success" then
    return "ready"
  end
  return "open"
end

---@param snap dots.Snapshot
---@param id string
---@return dots.Session?
local function session_by_id(snap, id)
  for _, session in ipairs(snap.sessions) do
    if session.id == id then
      return session
    end
  end
  return nil
end

---The label of the PR's session: the one its work belongs to while it is
---live, otherwise the one working at its checkout, if any.
---@param pr dots.WatchedPr
---@param snap dots.Snapshot
---@return string?
local function session_label(pr, snap)
  local session = pr.rex_session and session_by_id(snap, pr.rex_session)
  if not session then
    local id = pr.checkout and snapshot.session_at(snap, pr.checkout)
    session = id and session_by_id(snap, id)
  end
  return session and session.label or nil
end

---@param pr dots.WatchedPr
---@param session? string The label of the PR's session.
---@param dots string The dots command.
---@return pick.Row
local function row(pr, session, dots)
  local status = CLOSED[pr.state] or CI[pr.ci] or CI.unknown
  if pr.error and not CLOSED[pr.state] then
    status = { text = "! error", role = "red" }
  end
  local review = REVIEW[pr.review] or REVIEW.unknown
  if pr.state == "draft" then
    review = { text = "draft", role = "muted" }
  end
  local repo = pr.repository:match("/(.+)$") or pr.repository
  return {
    id = pr.url,
    group = M.group(pr),
    cells = {
      { text = session or "", role = "blue" },
      { text = repo .. "#" .. pr.number },
      pr.title and { text = pr.title } or { text = "not checked yet", role = "muted" },
      { text = status.text, role = status.role },
      { text = review.text, role = review.role },
    },
    search = table.concat({
      pr.repository,
      pr.head_ref or "",
      session or "",
      pr.error or "",
      pr.state,
      pr.review,
      pr.ci,
    }, " "),
    preview = { dots, "github", "watch", "show", "--color", pr.url },
  }
end

---The PRs the view shows: every PR not yet done, in list order, then the
---`DONE_LIMIT` most recently checked (or added) done ones, newest first.
---@param prs dots.WatchedPr[]
---@return dots.WatchedPr[]
function M.shown(prs)
  local shown, done = {}, {}
  for _, pr in ipairs(prs) do
    if M.group(pr) == "done" then
      done[#done + 1] = pr
    else
      shown[#shown + 1] = pr
    end
  end
  table.sort(done, function(a, b)
    local ta, tb = a.checked_at or a.added_at, b.checked_at or b.added_at
    if ta ~= tb then
      return ta > tb
    end
    return a.url < b.url
  end)
  for i = 1, math.min(#done, DONE_LIMIT) do
    shown[#shown + 1] = done[i]
  end
  return shown
end

---The view for a watch list. Pure: the store and Rex are read by the actions.
---@param list dots.WatchList
---@param snap dots.Snapshot For each PR's session.
---@param dots string The dots command, for previews and the watch.
---@return pick.View view
function M.view(list, snap, dots)
  local rows = {} ---@type pick.Row[]
  for _, pr in ipairs(M.shown(list.prs)) do
    rows[#rows + 1] = row(pr, session_label(pr, snap), dots)
  end
  local columns = {
    { width = { content = 24 } },
    { width = { content = 28 } },
    { width = "fill" },
    { width = { cells = 7 } },
    { width = { cells = 8 } },
  } ---@type pick.Column[]
  return {
    prompt = "prs› ",
    accept = "prs_open",
    shortcuts = {
      { key = "o", label = "link", action = "prs_link" },
      { key = "x", label = "unwatch", action = "prs_unwatch" },
    },
    groups = {
      { id = "attention", label = "needs attention", columns = columns },
      { id = "ready", label = "ready to merge", columns = columns },
      { id = "open", label = "open", columns = columns },
      { id = "done", label = "merged and closed", columns = columns },
    },
    rows = rows,
    watch = { dots, "github", "watch", "follow" },
    preview_anchor = "start",
    preview_position = "below",
  }
end

---@param args string[]
---@return string stdout
local function watch_cli(args)
  local argv = { "dots", "github", "watch" }
  for _, arg in ipairs(args) do
    argv[#argv + 1] = arg
  end
  local result = must("running dots github watch", process.run(argv, { timeout = 30 }))
  if not result.ok then
    error("dots github watch " .. args[1] .. ": " .. result.stderr, 0)
  end
  return result.stdout
end

---@return dots.WatchList
function M.list()
  return must("reading the watch list", rex.json.decode(watch_cli({ "list", "--json" })))
end

---@param url string
---@return dots.WatchedPr?
local function find(url)
  for _, pr in ipairs(M.list().prs) do
    if pr.url == url then
      return pr
    end
  end
  return nil
end

---Whether Rex still has the session.
---@param id string
---@return boolean
local function live(id)
  local listed = must("listing sessions", rex.call("session.list"))
  for _, entry in ipairs(listed.sessions or {}) do
    if entry.session_id == id then
      return true
    end
  end
  return false
end

---Go to the session the PR's work belongs to while it is live; otherwise to
---the session at the PR's checkout, creating one if needed; without a
---checkout that still exists, show its clickable link.
---@param ctx rex.Context
---@param args { id: string }
function M.open(ctx, args)
  local pr = find(args.id)
  local session = pr and pr.rex_session
  if session and live(session) then
    local from = jumps.here(ctx)
    app.show(session, nil, ctx.session_id)
    jumps.record(from, { session_id = session })
    return
  end
  local checkout = pr and pr.checkout
  if checkout then
    local exists = process.run({ "test", "-d", checkout }, { timeout = 5 })
    if exists and exists.ok then
      go.go_to(ctx, { id = "dir|" .. checkout })
      return
    end
  end
  views.link(ctx, args.id)
end

function M.install()
  rex.action({
    name = "prs",
    title = "Watched PRs…",
    description = "PRs agents are watching, with review and CI status",
    category = "Navigation",
    keywords = "pull requests prs github ci review watch",
    run = function(ctx)
      views.open({ title = "prs", view = "prs_view", at = ctx })
    end,
  })
  rex.action({
    name = "prs_view",
    title = "PRs: view",
    description = "The PR picker's rows, for dots-pick",
    category = "Navigation",
    run = function(ctx)
      local dots = process.which("dots") or error("dots is not installed on the command path", 0)
      return M.view(M.list(), snapshot.collect({ exclude = ctx.block_id }), dots)
    end,
  })
  rex.action({
    name = "prs_open",
    title = "PRs: go to a PR's work",
    category = "Navigation",
    args = { id = "string" },
    run = M.open,
  })
  rex.action({
    name = "prs_link",
    title = "PRs: show link",
    category = "Navigation",
    args = { id = "string" },
    run = function(ctx, args)
      views.link(ctx, args.id)
    end,
  })
  rex.action({
    name = "prs_unwatch",
    title = "PRs: stop watching",
    category = "Navigation",
    args = { id = "string" },
    run = function(_, args)
      watch_cli({ "remove", args.id })
    end,
  })
  rex.bind("cmd+shift+r", "prs")
end

return M
