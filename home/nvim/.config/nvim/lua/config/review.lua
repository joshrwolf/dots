-- `dots review` in CodeDiff: open a worktree's review (found or created and
-- brought up to date by the CLI) on the exact revisions of one of its presets,
-- find worktrees needing attention in an inbox across repositories, show its threads and the user's drafts as diagnostics and in the
-- quickfix list, and write drafts, submissions, statuses and reviewed marks
-- through the CLI, which owns the review files and every rule about them,
-- including where anchors fall in each preset. Agent changes appear live.
local M = {}

local codediff = require("codediff.api")

local author = "user"
local ns = vim.api.nvim_create_namespace("dots.review")
local draft_ns = vim.api.nvim_create_namespace("dots.review.drafts")
local group = vim.api.nvim_create_augroup("dots.review", { clear = true })
local views = {} -- tabpage -> open review view
local buffers = 0

-- Review anchors name sides old/new; CodeDiff names them original/modified.
local to_codediff = { old = "original", new = "modified" }
local from_codediff = { original = "old", modified = "new" }
-- Core file statuses as CodeDiff's Git letters.
local letters = { added = "A", modified = "M", deleted = "D", type_changed = "T" }

local function notify(message, level)
  vim.notify("Review: " .. message, level or vim.log.levels.INFO)
end

-- Run `dots --json review ARGS`; failures show the CLI's own error.
local function dots(args, opts, on_done, on_error)
  local function fail(message)
    notify(message, vim.log.levels.ERROR)
    if on_error then
      on_error()
    end
  end
  local ok, err = pcall(
    vim.system,
    vim.list_extend({ "dots", "--json", "review" }, args),
    { cwd = opts.cwd, stdin = opts.stdin, text = true, timeout = 10000 },
    vim.schedule_wrap(function(result)
      local decoded, out = pcall(vim.json.decode, result.stdout or "", { luanil = { object = true, array = true } })
      if result.code ~= 0 then
        local message = decoded and type(out) == "table" and out.error or vim.trim(result.stderr or "")
        fail(message ~= "" and message or "dots exited with code " .. result.code)
      elseif not decoded or type(out) ~= "table" then
        fail("unreadable output from dots review " .. args[1])
      else
        on_done(out)
      end
    end)
  )
  if not ok then
    fail(tostring(err))
  end
end

-- `show` as the viewer needs it: with drafts and one preset's view.
local function show_args(id, preset)
  local args = { "show", "--drafts", "--preset", preset }
  if id then
    table.insert(args, 2, id)
  end
  return args
end

-- Where repository commands run: the current file's directory or the cwd.
local function checkout()
  local view = views[vim.api.nvim_get_current_tabpage()]
  local file = vim.api.nvim_buf_get_name(0)
  return view and view.cwd or (vim.bo.buftype == "" and file ~= "" and vim.fs.dirname(file) or vim.fn.getcwd())
end

local function age(at)
  local seconds = math.max(os.time() - at, 0)
  if seconds < 3600 then
    return math.floor(seconds / 60) .. "m"
  elseif seconds < 86400 then
    return math.floor(seconds / 3600) .. "h"
  end
  return math.floor(seconds / 86400) .. "d"
end

local styles = {
  open = { "●", "DiagnosticWarn" },
  accepted = { "◆", "DiagnosticInfo" },
  fixed = { "✓", "DiagnosticOk" },
  dismissed = { "✗", "Comment" },
  posted = { "↗", "DiagnosticHint" },
  outdated = { "○", "Comment" },
}
local draft_icon = "✎"

local function style(thread)
  return unpack(styles[thread.status] or { "●", "Comment" })
end

-- Open threads rank by finding severity; settled ones recede to hints.
local severities = {
  critical = vim.diagnostic.severity.ERROR,
  important = vim.diagnostic.severity.WARN,
  minor = vim.diagnostic.severity.INFO,
  nit = vim.diagnostic.severity.HINT,
}

local function severity(thread)
  if thread.status ~= "open" then
    return vim.diagnostic.severity.HINT
  end
  return severities[thread.severity] or vim.diagnostic.severity.WARN
end

vim.diagnostic.config({
  underline = false,
  severity_sort = true,
  signs = true,
  virtual_text = {
    spacing = 2,
    prefix = function(diagnostic)
      return (styles[diagnostic.user_data.status] or { "●" })[1]
    end,
  },
}, ns)
-- Drafts are the user's pending comments: one quiet style, apart from threads.
vim.diagnostic.config({
  underline = false,
  signs = { text = { draft_icon, draft_icon, draft_icon, draft_icon } },
  virtual_text = { spacing = 2, prefix = draft_icon },
}, draft_ns)

local function first_line(body)
  return vim.split(body or "", "\n")[1]
end

-- The user's draft replies to each thread, by thread id.
local function draft_replies(view)
  local replies = {}
  for _, draft in ipairs(view.review.drafts or {}) do
    if draft.thread then
      replies[draft.thread] = (replies[draft.thread] or 0) + 1
    end
  end
  return replies
end

local function summary(view, thread)
  local first = thread.comments[1] or {}
  local replies = 0
  for i = 2, #thread.comments do
    replies = replies + (thread.comments[i].body and 1 or 0)
  end
  local text = thread.title or first_line(first.body)
  local count = replies > 0 and (" · %d repl%s"):format(replies, replies == 1 and "y" or "ies") or ""
  local pending = view and draft_replies(view)[thread.id]
  if pending then
    count = count .. (" · %d draft repl%s"):format(pending, pending == 1 and "y" or "ies")
  end
  return text .. " — " .. (first.author or "?") .. count
end

local function where(anchor)
  if not anchor then
    return "review"
  elseif not anchor.start then
    return anchor.path .. " (file)"
  end
  return ("%s:%d-%d %s"):format(anchor.path, anchor.start, anchor["end"], anchor.side)
end

local function location(view, thread)
  local text = where(thread.anchor)
  return thread.revision == view.revision and text or text .. " (revision " .. thread.revision .. ")"
end

local function markdown(view, thread)
  local lines =
    { ("# %s · %s%s"):format(thread.id, thread.status, thread.severity and " · " .. thread.severity or "") }
  table.insert(lines, "`" .. location(view, thread) .. "`")
  if thread.title then
    vim.list_extend(lines, { "", "**" .. thread.title .. "**" })
  end
  for _, comment in ipairs(thread.comments) do
    table.insert(lines, "")
    if comment.status then
      table.insert(lines, ("_%s set %s · %s ago_"):format(comment.author, comment.status, age(comment.at)))
    else
      table.insert(lines, ("**%s** · %s ago"):format(comment.author, age(comment.at)))
    end
    if comment.body then
      vim.list_extend(lines, vim.split(comment.body, "\n"))
    end
  end
  for _, draft in ipairs(view.review.drafts or {}) do
    if draft.thread == thread.id then
      vim.list_extend(lines, { "", "_" .. draft_icon .. " draft reply " .. draft.id .. " (not submitted)_" })
      vim.list_extend(lines, vim.split(draft.body, "\n"))
    end
  end
  return lines
end

-- The path and side a buffer shows in view, if it is one of the view's sides.
local function side_of(view, buf)
  local at = codediff.buffer(buf, view.tab)
  if not at then
    return nil
  end
  return at.path, from_codediff[at.side]
end

-- This tab's review view and the path and side the current buffer shows.
local function context()
  local view = views[vim.api.nvim_get_current_tabpage()]
  if not view then
    return nil
  end
  return view, side_of(view, vim.api.nvim_get_current_buf())
end

-- Navigation order: path, line, then the old side before the new one.
local function before(a, b)
  if a.path ~= b.path then
    return a.path < b.path
  elseif a.start ~= b.start then
    return a.start < b.start
  end
  return a.side == "old" and b.side == "new"
end

-- Where the core placed `id` in this view, in the view's own lines; a
-- whole-file anchor lands on line 1 of the side that has the file.
local function placed(view, id)
  local at = view.layout.anchors[id]
  if not at then
    return nil
  end
  if at.start then
    return { path = at.path, side = at.side, start = at.start, ["end"] = at["end"] }
  end
  local file = view.layout_files[at.path]
  local side = file and file.status == "deleted" and "old" or "new"
  return { path = at.path, side = side, start = 1, ["end"] = 1, file = true }
end

-- Threads and new-thread drafts this view shows, in navigation order:
-- { at, thread } or { at, draft }.
local function shown(view)
  local items = {}
  for _, thread in ipairs(view.review.threads) do
    local at = thread.anchor and thread.revision == view.revision and placed(view, thread.id)
    if at then
      table.insert(items, { at = at, thread = thread })
    end
  end
  for _, draft in ipairs(view.review.drafts or {}) do
    local at = not draft.thread and draft.anchor and draft.revision == view.revision and placed(view, draft.id)
    if at then
      table.insert(items, { at = at, draft = draft })
    end
  end
  table.sort(items, function(a, b)
    return before(a.at, b.at)
  end)
  return items
end

local function pending(view)
  return #(view.review.drafts or {})
end

local function list_title(view)
  local count = pending(view)
  return ("Review %s · %s%s"):format(
    view.id,
    view.preset,
    count > 0 and (" · %d draft%s"):format(count, count == 1 and "" or "s") or ""
  )
end

-- Per-file marks in the file list: open threads, drafts, reviewed (✓, with
-- the name dimmed) and changed since the user last looked (Δ).
local function decorations(view)
  local open, drafted = {}, {}
  for _, thread in ipairs(view.review.threads) do
    if thread.anchor and thread.status == "open" then
      open[thread.anchor.path] = (open[thread.anchor.path] or 0) + 1
    end
  end
  for _, draft in ipairs(view.review.drafts or {}) do
    if draft.anchor then
      drafted[draft.anchor.path] = (drafted[draft.anchor.path] or 0) + 1
    end
  end
  local out = {}
  for path in pairs(view.layout_files) do
    local file = view.files[path] or {}
    local parts, hl = {}, nil
    if open[path] then
      table.insert(parts, "● " .. open[path])
      hl = "DiagnosticWarn"
    end
    if drafted[path] then
      table.insert(parts, draft_icon .. " " .. drafted[path])
      hl = hl or "DiagnosticHint"
    end
    if view.changed[path] then
      table.insert(parts, "Δ")
      hl = hl or "DiagnosticInfo"
    end
    if file.reviewed then
      table.insert(parts, "✓")
      hl = hl or "DiagnosticOk"
    end
    if #parts > 0 then
      out[path] = { text = table.concat(parts, " "), hl = hl, name_hl = file.reviewed and "Comment" or nil }
    end
  end
  return out
end

local setup_buffer, reopen

-- Draw view's threads and drafts: diagnostics on its revision buffers (shown or
-- not), its quickfix list, and per-file marks in the file list. Reviews sharing
-- a revision share CodeDiff's buffers, so only the current tab's review is drawn.
local function render(view)
  local diagnostics, items = { [ns] = {}, [draft_ns] = {} }, {}
  for _, item in ipairs(shown(view)) do
    local at = item.at
    local buf = codediff.revision_buffer(view.tab, at.path, to_codediff[at.side])
    if buf then
      local namespace, entry, text
      if item.thread then
        local thread = item.thread
        text = (at.file and "file · " or "") .. thread.status .. " · " .. summary(view, thread)
        namespace = ns
        entry = {
          severity = severity(thread),
          code = thread.id,
          user_data = { thread = thread.id, status = thread.status },
        }
      else
        text = (at.file and "file · " or "") .. "draft · " .. first_line(item.draft.body)
        namespace = draft_ns
        entry = { severity = vim.diagnostic.severity.HINT, code = item.draft.id, user_data = { draft = item.draft.id } }
      end
      diagnostics[namespace][buf] = diagnostics[namespace][buf] or {}
      table.insert(
        diagnostics[namespace][buf],
        vim.tbl_extend("force", entry, {
          lnum = at.start - 1,
          end_lnum = at["end"] - 1,
          col = 0,
          message = text,
          source = "review",
        })
      )
      table.insert(items, {
        bufnr = buf,
        lnum = at.start,
        end_lnum = at["end"],
        text = ("%s %s (%s)"):format(entry.code, text, at.side),
        type = ({ "E", "W", "I", "N" })[entry.severity],
      })
    end
  end
  for namespace, by_buffer in pairs(diagnostics) do
    vim.diagnostic.reset(namespace)
    for buf, list in pairs(by_buffer) do
      vim.diagnostic.set(namespace, buf, list)
    end
  end
  vim.t[view.tab].dots_review_drafts = pending(view)
  if vim.fn.getqflist({ id = view.qf }).id == view.qf then
    vim.fn.setqflist({}, "u", { id = view.qf, items = items, title = list_title(view) })
  end
  codediff.set_decorations(view.tab, decorations(view))
  for _, win in ipairs(vim.api.nvim_tabpage_list_wins(view.tab)) do
    local buf = vim.api.nvim_win_get_buf(win)
    if side_of(view, buf) then
      setup_buffer(buf)
    end
  end
end

local function render_current()
  local view = views[vim.api.nvim_get_current_tabpage()]
  if view then
    render(view)
  end
end

-- Make view's quickfix list the current one, recreating it if the quickfix
-- stack has dropped it, so native list commands (:cc, :cnext) jump into this
-- tab's revisions rather than another review's.
local function select_list(view)
  local nr = vim.fn.getqflist({ id = view.qf or 0, nr = 0 }).nr
  if not view.qf or nr == 0 then
    vim.fn.setqflist({}, " ", { title = list_title(view), context = { dots_review = view.id } })
    view.qf = vim.fn.getqflist({ id = 0 }).id
  elseif nr ~= vim.fn.getqflist({ nr = 0 }).nr then
    vim.cmd("silent " .. nr .. "chistory")
  end
end

local function by_path(files)
  local out = {}
  for _, file in ipairs(files or {}) do
    out[file.path] = file
  end
  return out
end

-- Take in a `show` result: the review, its files and the preset's placements.
local function absorb(view, out)
  view.review = out.review
  view.files = by_path(out.files)
  view.layout = out.view
  view.layout_files = by_path(out.view.files)
end

local function reload(view)
  view.generation = view.generation + 1
  local generation = view.generation
  dots(show_args(view.id, view.preset), { cwd = view.cwd }, function(out)
    if views[view.tab] ~= view or generation ~= view.generation then
      return
    end
    if #out.review.revisions - 1 ~= view.revision then
      if not view.stale then
        view.stale = true
        notify(view.id .. " moved to a new revision; reopen it to see its current threads", vim.log.levels.WARN)
      end
      -- Placements and files describe the new revision; keep the ones this
      -- view shows.
      out.view = view.layout
      out.files = vim.tbl_values(view.files)
    end
    absorb(view, out)
    if view.tab == vim.api.nvim_get_current_tabpage() then
      render(view)
    end
  end)
end

-- Reviews are replaced by atomic renames, so watch the directory, not the file.
local function watch(view)
  local name = vim.fs.basename(view.file)
  local reload_view = vim.schedule_wrap(function()
    reload(view)
  end)
  view.timer = assert(vim.uv.new_timer())
  view.watcher = assert(vim.uv.new_fs_event())
  view.watcher:start(vim.fs.dirname(view.file), {}, function(err, filename)
    if err or (filename and vim.fs.basename(filename) ~= name) then
      return
    end
    view.timer:stop()
    view.timer:start(100, 0, reload_view)
  end)
end

-- Record that the user looked at the revision this view shows; the core
-- keeps a newer look.
local function look(view)
  dots({ "look", view.id, "--revision", tostring(view.revision) }, { cwd = view.cwd }, function() end)
end

local function close(view)
  views[view.tab] = nil
  for _, handle in ipairs({ view.watcher, view.timer }) do
    if not handle:is_closing() then
      handle:close()
    end
  end
  look(view)
  vim.diagnostic.reset(ns)
  vim.diagnostic.reset(draft_ns)
  render_current()
end

local function jump_to(view, at)
  local ok, err = codediff.show(
    view.tab,
    { path = at.path, side = to_codediff[at.side], line = at.start },
    function(win)
      vim.api.nvim_win_call(win, function()
        vim.cmd("normal! zz")
      end)
    end
  )
  if not ok then
    notify(err, vim.log.levels.WARN)
  end
end

function M.next_thread(direction)
  local view, path, side = context()
  if not view then
    return notify("no review in this tab", vim.log.levels.WARN)
  end
  local items = shown(view)
  if #items == 0 then
    return notify("no threads in this view")
  end
  local cursor = { path = path or "", start = vim.fn.line("."), side = side }
  local ahead = vim.tbl_filter(function(item)
    if direction > 0 then
      return before(cursor, item.at)
    end
    return before(item.at, cursor)
  end, items)
  -- Wrap around at either end.
  local item = direction > 0 and (ahead[1] or items[1]) or (ahead[#ahead] or items[#items])
  jump_to(view, item.at)
end

local function open_float(buf, opts)
  opts.width = math.min(opts.width, vim.o.columns - 8)
  return vim.api.nvim_open_win(
    buf,
    true,
    vim.tbl_extend("force", {
      relative = "editor",
      row = math.floor((vim.o.lines - opts.height) / 2),
      col = math.floor((vim.o.columns - opts.width) / 2),
      border = "rounded",
      footer_pos = "right",
    }, opts)
  )
end

-- A Markdown buffer that submits on write (:w, :x, ZZ) and is dropped by
-- closing it unwritten (ZQ, :q!); plain :q asks first ('confirm'). An empty
-- body is dropped unless `opts.empty` allows it.
local function compose(title, on_submit, text, opts)
  opts = opts or {}
  buffers = buffers + 1
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_name(buf, ("dots-review://draft/%d"):format(buffers))
  vim.bo[buf].buftype = "acwrite"
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].filetype = "markdown"
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, vim.split(text or "", "\n"))
  vim.bo[buf].modified = true
  vim.api.nvim_create_autocmd("BufWriteCmd", {
    buffer = buf,
    callback = function()
      local body = vim.trim(table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), "\n"))
      vim.bo[buf].modified = false
      -- After :x or ZZ has closed the window itself.
      vim.schedule(function()
        local win = vim.fn.bufwinid(buf)
        if win ~= -1 then
          vim.api.nvim_win_close(win, true)
        end
        if body == "" and not opts.empty then
          return notify("empty comment discarded")
        end
        on_submit(body)
      end)
    end,
  })
  vim.api.nvim_create_autocmd("BufWipeout", {
    buffer = buf,
    callback = function()
      if vim.bo[buf].modified then
        notify(opts.dropped or "comment discarded")
      end
    end,
  })
  local footer = opts.footer or " :w save draft · ZQ discard "
  local win = open_float(buf, { width = 80, height = 10, title = " " .. title .. " ", footer = footer })
  vim.wo[win].wrap = true
  vim.cmd.startinsert()
  return buf
end

-- Lines of an older snapshot cannot anchor at the review's current revision.
local function refuse_stale(view, body, what)
  view.stale = true
  if body then
    vim.fn.setreg('"', body)
  end
  notify(
    view.id
      .. " has moved past the revision shown here; reopen it (<leader>gr or <leader>fR) to "
      .. (what or "comment")
      .. (body and '. Your draft is in the " register.' or "."),
    vim.log.levels.ERROR
  )
end

local function drafted(view, out)
  local count = pending(view) + 1
  notify(("drafted %s · %d pending; <leader>rS submits"):format(out.draft.id, count))
  reload(view)
end

-- Draft a new thread at `anchor` (view lines; nil lines for the whole file).
local function draft_thread(view, anchor, title)
  local function submit(body)
    if view.stale then
      return refuse_stale(view, body)
    end
    -- The CLI maps the view's lines into the review's and refuses the draft
    -- if the review moved since this view's revision.
    local args = { "draft", view.id, "--path", anchor.path, "--preset", view.preset }
    if anchor.start then
      vim.list_extend(
        args,
        { "--side", anchor.side, "--start", tostring(anchor.start), "--end", tostring(anchor["end"]) }
      )
    end
    vim.list_extend(args, { "--revision", tostring(view.revision), "--body-file", "-" })
    dots(args, { cwd = view.cwd, stdin = body }, function(out)
      drafted(view, out)
    end, function()
      -- A moved review cannot take this draft; any other refusal can be edited.
      dots({ "show", view.id }, { cwd = view.cwd }, function(out)
        if #out.review.revisions - 1 ~= view.revision then
          return refuse_stale(view, body)
        end
        compose(title, submit, body)
      end, function()
        compose(title, submit, body)
      end)
    end)
  end
  return compose(title, submit)
end

function M.comment(line1, line2)
  local view, path, side = context()
  if not side then
    return notify("not in a review diff buffer", vim.log.levels.WARN)
  elseif view.stale then
    return refuse_stale(view)
  end
  local anchor = { path = path, side = side, start = math.min(line1, line2), ["end"] = math.max(line1, line2) }
  return draft_thread(view, anchor, ("Comment on %s:%d-%d (%s)"):format(path, anchor.start, anchor["end"], side))
end

function M.comment_file()
  local view, path = context()
  if not path then
    return notify("not in a review diff buffer", vim.log.levels.WARN)
  elseif view.stale then
    return refuse_stale(view)
  end
  return draft_thread(view, { path = path }, "Comment on " .. path)
end

function M.reply(view, thread)
  local title = "Reply to " .. thread.id
  local function submit(body)
    dots(
      { "draft", view.id, "--thread", thread.id, "--body-file", "-" },
      { cwd = view.cwd, stdin = body },
      function(out)
        drafted(view, out)
      end,
      function()
        compose(title, submit, body)
      end
    )
  end
  return compose(title, submit)
end

-- Status changes take effect at once; only comments wait for a submission.
function M.set_status(view, thread)
  vim.ui.select({ "open", "accepted", "dismissed", "fixed" }, { prompt = "Status of " .. thread.id }, function(status)
    if status then
      dots({ "set", view.id, thread.id, "--status", status, "--author", author }, { cwd = view.cwd }, function()
        reload(view)
      end)
    end
  end)
end

-- Publish every draft at once, with an optional summary written in a buffer.
function M.submit()
  local view = views[vim.api.nvim_get_current_tabpage()]
  if not view then
    return notify("no review in this tab", vim.log.levels.WARN)
  end
  local count = pending(view)
  local function submit(body)
    if body == "" and count == 0 then
      return notify("nothing to submit: no drafts and no summary")
    end
    local args = { "submit", view.id }
    if body ~= "" then
      vim.list_extend(args, { "--body-file", "-" })
    end
    dots(args, { cwd = view.cwd, stdin = body ~= "" and body or nil }, function(out)
      notify(("submitted %d comment%s"):format(#out.submission.comments, #out.submission.comments == 1 and "" or "s"))
      reload(view)
    end, function()
      notify("<leader>rl lists every draft, including stale ones, to discard", vim.log.levels.WARN)
      reload(view)
    end)
  end
  return compose(
    ("Submit %d draft%s · summary (optional)"):format(count, count == 1 and "" or "s"),
    submit,
    nil,
    { empty = true, footer = " :w submit · ZQ cancel ", dropped = "submission cancelled" }
  )
end

function M.discard(view, draft)
  dots({ "discard", view.id, "--draft", draft.id }, { cwd = view.cwd }, function()
    notify("discarded " .. draft.id)
    reload(view)
  end)
end

function M.discard_all()
  local view = views[vim.api.nvim_get_current_tabpage()]
  if not view then
    return notify("no review in this tab", vim.log.levels.WARN)
  end
  local count = pending(view)
  if count == 0 then
    return notify("no drafts")
  end
  vim.ui.select(
    { "Discard", "Keep" },
    { prompt = ("Discard %d draft%s?"):format(count, count == 1 and "" or "s") },
    function(choice)
      if choice == "Discard" then
        dots({ "discard", view.id, "--all" }, { cwd = view.cwd }, function()
          notify("discarded every draft")
          reload(view)
        end)
      end
    end
  )
end

-- Mark the current file reviewed, or clear its mark.
function M.toggle_reviewed()
  local view, path = context()
  if not path then
    return notify("not in a review diff buffer", vim.log.levels.WARN)
  end
  local file = view.files[path]
  if view.stale then
    return refuse_stale(view, nil, "mark files")
  elseif not file then
    return notify(path .. " is not changed in this review", vim.log.levels.WARN)
  end
  -- The core refuses if the review moved past what this view shows.
  local args = { file.reviewed and "unmark" or "mark", view.id, path, "--revision", tostring(view.revision) }
  dots(args, { cwd = view.cwd }, function(out)
    notify(path .. (out.file.reviewed and " marked reviewed" or " no longer marked reviewed"))
    reload(view)
  end, function()
    reload(view)
  end)
end

-- Reopen this tab's review in the next preset the core offers.
function M.next_preset()
  local view = views[vim.api.nvim_get_current_tabpage()]
  if not view then
    return notify("no review in this tab", vim.log.levels.WARN)
  end
  local available = view.layout.available
  local index = 1
  for i, preset in ipairs(available) do
    if preset == view.preset then
      index = i
    end
  end
  local next = available[index % #available + 1]
  if next == view.preset then
    return notify("only the " .. view.preset .. " view has changes")
  end
  reopen(view.id, view.cwd, next)
end

function M.show_thread(view, thread)
  local lines = markdown(view, thread)
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].filetype = "markdown"
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].modifiable = false
  local height = math.min(#lines, math.floor(vim.o.lines * 0.6))
  local win = open_float(buf, { width = 90, height = height, footer = " r reply · s status · q close " })
  vim.wo[win].wrap = true
  local function act(action)
    return function()
      vim.api.nvim_win_close(win, true)
      action(view, thread)
    end
  end
  vim.keymap.set("n", "q", act(function() end), { buffer = buf, desc = "Close thread" })
  vim.keymap.set("n", "r", act(M.reply), { buffer = buf, desc = "Reply to thread (draft)" })
  vim.keymap.set("n", "s", act(M.set_status), { buffer = buf, desc = "Set thread status" })
end

-- Run action on the thread (or, with `drafts`, the draft) under the cursor,
-- choosing when several overlap.
local function at_cursor(action, drafts)
  return function()
    local view, path, side = context()
    if not side then
      return notify("not in a review diff buffer", vim.log.levels.WARN)
    end
    local line = vim.fn.line(".")
    local here = vim.tbl_filter(function(item)
      local at = item.at
      return (drafts and item.draft or item.thread)
        and at.path == path
        and at.side == side
        and at.start <= line
        and line <= at["end"]
    end, shown(view))
    local kind = drafts and "draft" or "thread"
    if #here == 0 then
      return notify("no " .. kind .. " on this line")
    elseif #here == 1 then
      return action(view, here[1].thread or here[1].draft)
    end
    vim.ui.select(here, {
      prompt = drafts and "Draft" or "Thread",
      format_item = function(item)
        return item.thread and item.thread.id .. " " .. summary(view, item.thread)
          or item.draft.id .. " " .. first_line(item.draft.body)
      end,
    }, function(item)
      if item then
        action(view, item.thread or item.draft)
      end
    end)
  end
end

function setup_buffer(buf)
  if vim.b[buf].dots_review then
    return
  end
  vim.b[buf].dots_review = true
  local function map(mode, lhs, rhs, desc)
    vim.keymap.set(mode, lhs, rhs, { buffer = buf, desc = desc })
  end
  map("n", "]r", function()
    M.next_thread(1)
  end, "Next review thread or draft")
  map("n", "[r", function()
    M.next_thread(-1)
  end, "Previous review thread or draft")
  map("n", "<leader>rc", function()
    M.comment(vim.fn.line("."), vim.fn.line("."))
  end, "Comment on line (draft)")
  map("x", "<leader>rc", function()
    local start, finish = vim.fn.line("v"), vim.fn.line(".")
    vim.cmd("normal! \27")
    M.comment(start, finish)
  end, "Comment on selection (draft)")
  map("n", "<leader>rf", M.comment_file, "Comment on file (draft)")
  map("n", "<leader>rr", at_cursor(M.reply), "Reply to thread (draft)")
  map("n", "<leader>rs", at_cursor(M.set_status), "Set thread status")
  map("n", "<leader>rv", at_cursor(M.show_thread), "View thread")
  map("n", "<leader>rd", at_cursor(M.discard, true), "Discard draft on line")
  map("n", "<leader>rD", M.discard_all, "Discard every draft")
  map("n", "<leader>rS", M.submit, "Submit drafts")
  map("n", "<leader>rl", M.pick_drafts, "List pending drafts")
  map("n", "<leader>rm", M.toggle_reviewed, "Toggle file reviewed")
  map("n", "<leader>rp", M.next_preset, "Next review view (everything/uncommitted/committed)")
end

-- Show `out`, a show or open result with a view of `preset`, in a new tab
-- whose working directory is the review's worktree. A tab already showing
-- that revision in `preset` is reused; one showing it in another preset is
-- replaced.
local function present(out, preset)
  local review = out.review
  local revision = review.revisions[#review.revisions]
  if #out.view.files == 0 then
    return notify("nothing to review in " .. review.worktree.path .. " against " .. revision.basis.reference)
  end
  local replaced
  for tab, view in pairs(views) do
    if view.id == review.id and view.snapshot == revision.snapshot and vim.api.nvim_tabpage_is_valid(tab) then
      if view.preset == preset then
        return vim.api.nvim_set_current_tabpage(tab)
      end
      replaced = tab
    end
  end
  local files = {}
  for _, file in ipairs(out.view.files) do
    table.insert(files, { path = file.path, status = letters[file.status] })
  end
  local root = review.worktree.path
  local tab, err = codediff.open({ root = root, base = out.view.base, head = out.view.head, files = files })
  if not tab then
    return notify(err, vim.log.levels.ERROR)
  end
  if vim.api.nvim_get_current_tabpage() == tab then
    vim.cmd.tcd(vim.fn.fnameescape(root))
  end
  local changed = {}
  for _, file in ipairs(out.files) do
    changed[file.path] = file.changed_since_looked or nil
  end
  local view = {
    id = review.id,
    tab = tab,
    cwd = root,
    file = out.file,
    revision = #review.revisions - 1,
    snapshot = revision.snapshot,
    preset = preset,
    -- What changed since the user last looked, as of opening: looking now
    -- moves the mark, so the view keeps this set.
    changed = changed,
    generation = 0,
  }
  absorb(view, out)
  views[tab] = view
  select_list(view)
  watch(view)
  look(view)
  render_current()
  if replaced then
    local current = vim.api.nvim_get_current_tabpage()
    vim.api.nvim_set_current_tabpage(replaced)
    vim.cmd.tabclose()
    vim.api.nvim_set_current_tabpage(current)
  end
end

-- Reopen review `id` on its newest revision in `preset`.
function reopen(id, cwd, preset)
  dots(show_args(id, preset), { cwd = cwd }, function(out)
    present(out, preset)
  end)
end

-- Open the review of the worktree containing `path` (default: the current
-- file's or the working directory), creating it or bringing it up to date.
function M.open(path)
  dots({ "open", "--drafts", "--preset", "everything" }, { cwd = path or checkout() }, function(out)
    present(out, "everything")
  end)
end

-- Every known repository's worktrees with changes or reviews, most in need of
-- attention first; choosing one opens its review.
function M.inbox()
  dots({ "inbox" }, { cwd = checkout() }, function(out)
    for _, skipped in ipairs(out.skipped) do
      notify(("skipped %s: %s"):format(skipped.path, skipped.reason), vim.log.levels.WARN)
    end
    if #out.worktrees == 0 then
      return notify("no worktrees with changes or reviews")
    end
    local items = {}
    for _, entry in ipairs(out.worktrees) do
      local review = entry.review
      local name = entry.branch or vim.fs.basename(entry.path)
      local repository = vim.fs.basename(vim.fs.dirname(entry.repository))
      local lines = {
        "# " .. name,
        "`" .. entry.path .. "`",
        "",
        ("Base **%s** (%s)"):format(entry.basis.reference, entry.basis.reason),
        ("%d files · +%d −%d"):format(entry.diff.files, entry.diff.added, entry.diff.removed),
      }
      if review then
        vim.list_extend(lines, {
          "",
          ("Review %s · owner %s"):format(review.id, review.owner or "none"),
          "Waiting on you: " .. (#review.waiting > 0 and table.concat(review.waiting, ", ") or "nothing"),
          ("Drafts: %d"):format(review.drafts),
        })
        if review.changed_since_looked then
          table.insert(lines, "Changed since you looked:")
          for _, path in ipairs(review.changed_since_looked) do
            table.insert(lines, "- " .. path)
          end
        else
          table.insert(lines, "Not looked at yet")
        end
      else
        vim.list_extend(lines, { "", "No review yet; opening creates it" })
      end
      table.insert(items, {
        text = table.concat({ repository, name, entry.path, review and review.owner or "" }, " "),
        entry = entry,
        name = name,
        repository = repository,
        preview = { text = table.concat(lines, "\n"), ft = "markdown" },
      })
    end
    Snacks.picker.pick({
      title = "Review inbox",
      items = items,
      preview = "preview",
      format = function(item)
        local entry, review = item.entry, item.entry.review
        local waiting = review and #review.waiting or 0
        local drafts = review and review.drafts or 0
        local changed = review and review.changed_since_looked
        local delta = not review and "" or changed and (#changed > 0 and "Δ " .. #changed or "") or "new"
        return {
          { ("%-10s "):format(item.repository), "Comment" },
          { ("%-32s "):format(item.name), "Directory" },
          { ("%2d ● "):format(waiting), waiting > 0 and "DiagnosticWarn" or "Comment" },
          { ("%2d %s "):format(drafts, draft_icon), drafts > 0 and "DiagnosticHint" or "Comment" },
          { ("%-5s "):format(delta), "DiagnosticInfo" },
          { ("%3d files +%-5d -%-5d "):format(entry.diff.files, entry.diff.added, entry.diff.removed), "Comment" },
          { review and review.owner or "" },
        }
      end,
      confirm = function(picker, item)
        picker:close()
        if item then
          M.open(item.entry.path)
        end
      end,
    })
  end)
end

function M.pick_threads()
  local view = views[vim.api.nvim_get_current_tabpage()]
  if not view then
    return notify("no review in this tab", vim.log.levels.WARN)
  end
  local items = {}
  for _, thread in ipairs(view.review.threads) do
    local text = location(view, thread)
    table.insert(items, {
      text = table.concat({ thread.id, thread.status, text, summary(view, thread) }, " "),
      thread = thread,
      where = text,
      preview = { text = table.concat(markdown(view, thread), "\n"), ft = "markdown" },
    })
  end
  return Snacks.picker.pick({
    title = "Review threads",
    items = items,
    preview = "preview",
    format = function(item)
      local icon, hl = style(item.thread)
      return {
        { icon .. " " .. ("%-4s"):format(item.thread.id), hl },
        { ("%-10s"):format(item.thread.status), hl },
        { item.where .. "  ", "Directory" },
        { summary(view, item.thread) },
      }
    end,
    confirm = function(picker, item)
      picker:close()
      if not item then
        return
      end
      local at = item.thread.anchor and item.thread.revision == view.revision and placed(view, item.thread.id)
      if at then
        jump_to(view, at)
      else
        M.show_thread(view, item.thread)
      end
    end,
  })
end

-- Every pending draft, wherever it stands: placed in this view, outside it,
-- or left behind at an older revision by a refresh (which blocks submit).
-- <c-x> (or dd in the list) discards the selected draft.
function M.pick_drafts()
  local view = views[vim.api.nvim_get_current_tabpage()]
  if not view then
    return notify("no review in this tab", vim.log.levels.WARN)
  end
  local current = #view.review.revisions - 1
  local items = {}
  for _, draft in ipairs(view.review.drafts or {}) do
    local state
    if draft.thread then
      state = "reply to " .. draft.thread
    elseif draft.revision ~= current then
      state = ("stale at revision %d; cannot be submitted"):format(draft.revision)
    elseif draft.anchor and draft.revision == view.revision and not placed(view, draft.id) then
      state = "not in the " .. view.preset .. " view"
    end
    local text = where(draft.anchor)
    table.insert(items, {
      text = table.concat({ draft.id, text, state or "", first_line(draft.body) }, " "),
      draft = draft,
      where = text,
      state = state,
      preview = {
        text = table.concat(
          vim.list_extend(
            { "# " .. draft.id .. " · " .. text, state and "_" .. state .. "_" or "", "" },
            vim.split(draft.body, "\n")
          ),
          "\n"
        ),
        ft = "markdown",
      },
    })
  end
  if #items == 0 then
    return notify("no drafts")
  end
  return Snacks.picker.pick({
    title = "Review drafts · <c-x> discard",
    items = items,
    preview = "preview",
    format = function(item)
      return {
        { draft_icon .. " " .. ("%-4s"):format(item.draft.id), "DiagnosticHint" },
        { item.where .. "  ", "Directory" },
        item.state and { item.state .. "  ", item.state:find("^stale") and "DiagnosticError" or "Comment" } or { "" },
        { first_line(item.draft.body) },
      }
    end,
    actions = {
      discard = function(picker, item)
        picker:close()
        if item then
          M.discard(view, item.draft)
        end
      end,
    },
    win = {
      input = { keys = { ["<c-x>"] = { "discard", mode = { "n", "i" } } } },
      list = { keys = { ["dd"] = "discard" } },
    },
    confirm = function(picker, item)
      picker:close()
      if not item then
        return
      end
      local draft = item.draft
      local at = draft.thread and nil or (draft.revision == view.revision and placed(view, draft.id))
      local thread = draft.thread
        and vim.iter(view.review.threads):find(function(thread)
          return thread.id == draft.thread
        end)
      if at then
        jump_to(view, at)
      elseif thread then
        M.show_thread(view, thread)
      else
        notify(draft.id .. (item.state and ": " .. item.state or " is not placed in this view"))
      end
    end,
  })
end

vim.api.nvim_create_autocmd("User", {
  group = group,
  pattern = "CodeDiffClose",
  callback = function(event)
    local view = views[event.data.tabpage]
    if view then
      close(view)
    end
  end,
})

-- Content arriving resets a buffer's diagnostic marks to its first line, and
-- CodeDiff wipes revision buffers it stops showing (the quickfix list points at
-- buffer numbers), so redraw on both. Entering a tab draws its own review.
vim.api.nvim_create_autocmd("User", {
  group = group,
  pattern = "CodeDiffVirtualFileLoaded",
  callback = render_current,
})
vim.api.nvim_create_autocmd({ "BufWinEnter", "BufWipeout" }, {
  group = group,
  pattern = "codediff:///*",
  callback = vim.schedule_wrap(render_current),
})
vim.api.nvim_create_autocmd("TabEnter", {
  group = group,
  callback = function()
    local view = views[vim.api.nvim_get_current_tabpage()]
    if view then
      select_list(view)
      render(view)
    end
  end,
})

return M
