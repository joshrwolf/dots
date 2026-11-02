-- `dots review` in CodeDiff: open a review on its exact revisions, show its
-- threads as diagnostics and in the quickfix list, and write comments,
-- replies and statuses through the CLI, which owns the review files. Agent
-- changes appear live.
local M = {}

local codediff = require("codediff.api")

local author = "user"
local ns = vim.api.nvim_create_namespace("dots.review")
local group = vim.api.nvim_create_augroup("dots.review", { clear = true })
local views = {} -- tabpage -> open review view
local drafts = 0

-- Review anchors name sides old/new; CodeDiff names them original/modified.
local to_codediff = { old = "original", new = "modified" }
local from_codediff = { original = "old", modified = "new" }

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

-- The files a revision changes, with the status CodeDiff needs to show one-sided
-- files. Matches the review's own patch, which also ignores renames.
local function changed_files(cwd, base, snapshot, on_done)
  vim.system(
    { "git", "diff", "--no-renames", "--name-status", "-z", base, snapshot },
    { cwd = cwd, text = true },
    vim.schedule_wrap(function(result)
      if result.code ~= 0 then
        return notify(vim.trim(result.stderr or "git diff failed"), vim.log.levels.ERROR)
      end
      local fields, files = vim.split(result.stdout, "\0", { trimempty = true }), {}
      for i = 1, #fields - 1, 2 do
        table.insert(files, { status = fields[i]:sub(1, 1), path = fields[i + 1] })
      end
      on_done(files)
    end)
  )
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

local function target_text(target)
  if target.kind == "base" then
    return "base " .. target.reference
  elseif target.kind == "range" then
    return target.from .. ".." .. target.to
  end
  return target.kind
end

local styles = {
  open = { "●", "DiagnosticWarn" },
  accepted = { "◆", "DiagnosticInfo" },
  fixed = { "✓", "DiagnosticOk" },
  dismissed = { "✗", "Comment" },
  posted = { "↗", "DiagnosticHint" },
  outdated = { "○", "Comment" },
}

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

local function summary(thread)
  local first = thread.comments[1] or {}
  local replies = 0
  for i = 2, #thread.comments do
    replies = replies + (thread.comments[i].body and 1 or 0)
  end
  local text = thread.title or vim.split(first.body or "", "\n")[1]
  local count = replies > 0 and (" · %d repl%s"):format(replies, replies == 1 and "y" or "ies") or ""
  return text .. " — " .. (first.author or "?") .. count
end

local function location(view, thread)
  local a = thread.anchor
  local where = a and ("%s:%d-%d %s"):format(a.path, a.start, a["end"], a.side) or "review"
  return thread.revision == view.revision and where or where .. " (revision " .. thread.revision .. ")"
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

-- Anchored threads at the viewed revision, in navigation order.
local function anchored(view)
  local threads = vim.tbl_filter(function(thread)
    return thread.anchor ~= nil and thread.revision == view.revision
  end, view.review.threads)
  table.sort(threads, function(a, b)
    return before(a.anchor, b.anchor)
  end)
  return threads
end

local setup_buffer

-- Draw view's threads: diagnostics on its revision buffers (shown or not), its
-- quickfix list, and open-thread counts in the file list. Reviews sharing a
-- base share CodeDiff's buffers, so only the current tab's review is drawn.
local function render(view)
  local diagnostics, items, open = {}, {}, {}
  for _, thread in ipairs(anchored(view)) do
    local a = thread.anchor
    local buf = codediff.revision_buffer(view.tab, a.path, to_codediff[a.side])
    if buf then
      local message = thread.status .. " · " .. summary(thread)
      diagnostics[buf] = diagnostics[buf] or {}
      table.insert(diagnostics[buf], {
        lnum = a.start - 1,
        end_lnum = a["end"] - 1,
        col = 0,
        severity = severity(thread),
        message = message,
        source = "review",
        code = thread.id,
        user_data = { thread = thread.id, status = thread.status },
      })
      table.insert(items, {
        bufnr = buf,
        lnum = a.start,
        end_lnum = a["end"],
        text = ("%s %s (%s)"):format(thread.id, message, a.side),
        type = ({ "E", "W", "I", "N" })[severity(thread)],
      })
    end
    if thread.status == "open" then
      open[a.path] = (open[a.path] or 0) + 1
    end
  end
  vim.diagnostic.reset(ns)
  for buf, list in pairs(diagnostics) do
    vim.diagnostic.set(ns, buf, list)
  end
  if view.qf and vim.fn.getqflist({ id = view.qf }).id == view.qf then
    vim.fn.setqflist({}, "u", { id = view.qf, items = items })
  end
  local decorations = {}
  for path, count in pairs(open) do
    decorations[path] = { text = "● " .. count, hl = "DiagnosticWarn" }
  end
  codediff.set_decorations(view.tab, decorations)
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

local function reload(view)
  view.generation = view.generation + 1
  local generation = view.generation
  dots({ "show", view.id }, { cwd = view.cwd }, function(out)
    if views[view.tab] ~= view or generation ~= view.generation then
      return
    end
    view.review = out.review
    if #out.review.revisions - 1 ~= view.revision and not view.stale then
      view.stale = true
      notify(view.id .. " moved to a new revision; reopen it to see its current threads", vim.log.levels.WARN)
    end
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

local function close(view)
  views[view.tab] = nil
  for _, handle in ipairs({ view.watcher, view.timer }) do
    if not handle:is_closing() then
      handle:close()
    end
  end
  vim.diagnostic.reset(ns)
  render_current()
end

local function jump_to(view, thread)
  local a = thread.anchor
  local ok, err = codediff.show(view.tab, { path = a.path, side = to_codediff[a.side], line = a.start }, function(win)
    vim.api.nvim_win_call(win, function()
      vim.cmd("normal! zz")
    end)
  end)
  if not ok then
    notify(err, vim.log.levels.WARN)
  end
end

function M.next_thread(direction)
  local view, path, side = context()
  if not view then
    return notify("no review in this tab", vim.log.levels.WARN)
  end
  local threads = anchored(view)
  if #threads == 0 then
    return notify("no threads at this revision")
  end
  local cursor = { path = path or "", start = vim.fn.line("."), side = side }
  local ahead = vim.tbl_filter(function(thread)
    if direction > 0 then
      return before(cursor, thread.anchor)
    end
    return before(thread.anchor, cursor)
  end, threads)
  -- Wrap around at either end.
  jump_to(view, direction > 0 and (ahead[1] or threads[1]) or (ahead[#ahead] or threads[#threads]))
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

-- A Markdown draft that submits on write (:w, :x, ZZ) and is discarded by
-- closing it unwritten (ZQ, :q!); plain :q asks first ('confirm'). An
-- empty draft submits nothing.
local function compose(title, on_submit, text)
  drafts = drafts + 1
  local buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_name(buf, ("dots-review://draft/%d"):format(drafts))
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
        if body == "" then
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
        notify("comment discarded")
      end
    end,
  })
  local win =
    open_float(buf, { width = 80, height = 10, title = " " .. title .. " ", footer = " :w submit · ZQ discard " })
  vim.wo[win].wrap = true
  vim.cmd.startinsert()
  return buf
end

-- Lines of an older snapshot cannot anchor at the review's current revision.
local function refuse_stale(view, body)
  view.stale = true
  if body then
    vim.fn.setreg('"', body)
  end
  notify(
    view.id
      .. " has moved past the revision shown here; reopen it (<leader>gr or <leader>fR) to comment"
      .. (body and '. Your draft is in the " register.' or "."),
    vim.log.levels.ERROR
  )
end

function M.comment(line1, line2)
  local view, path, side = context()
  if not side then
    return notify("not in a review diff buffer", vim.log.levels.WARN)
  elseif view.stale then
    return refuse_stale(view)
  end
  local anchor = { path = path, side = side, start = math.min(line1, line2), ["end"] = math.max(line1, line2) }
  local title = ("Comment on %s:%d-%d (%s)"):format(path, anchor.start, anchor["end"], side)
  local function submit(body)
    if view.stale then
      return refuse_stale(view, body)
    end
    local stdin = vim.json.encode({ threads = { { body = body, author = author, anchor = anchor } } })
    -- The CLI refuses the batch if the review moved since this view's revision.
    local args = { "add", view.id, "--threads", "-", "--revision", tostring(view.revision) }
    dots(args, { cwd = view.cwd, stdin = stdin }, function(out)
      notify("opened " .. out.added[1].id)
      reload(view)
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

function M.reply(view, thread)
  local title = "Reply to " .. thread.id
  local function submit(body)
    dots(
      { "reply", view.id, thread.id, "--author", author, "--body-file", "-" },
      { cwd = view.cwd, stdin = body },
      function()
        reload(view)
      end,
      function()
        compose(title, submit, body)
      end
    )
  end
  return compose(title, submit)
end

function M.set_status(view, thread)
  vim.ui.select({ "open", "accepted", "dismissed", "fixed" }, { prompt = "Status of " .. thread.id }, function(status)
    if status then
      dots({ "set", view.id, thread.id, "--status", status, "--author", author }, { cwd = view.cwd }, function()
        reload(view)
      end)
    end
  end)
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
  vim.keymap.set("n", "r", act(M.reply), { buffer = buf, desc = "Reply to thread" })
  vim.keymap.set("n", "s", act(M.set_status), { buffer = buf, desc = "Set thread status" })
end

-- Run action on the thread under the cursor, choosing when several overlap.
local function at_cursor(action)
  return function()
    local view, path, side = context()
    if not side then
      return notify("not in a review diff buffer", vim.log.levels.WARN)
    end
    local line = vim.fn.line(".")
    local here = vim.tbl_filter(function(thread)
      local a = thread.anchor
      return a.path == path and a.side == side and a.start <= line and line <= a["end"]
    end, anchored(view))
    if #here == 0 then
      return notify("no thread on this line")
    elseif #here == 1 then
      return action(view, here[1])
    end
    vim.ui.select(here, {
      prompt = "Thread",
      format_item = function(thread)
        return thread.id .. " " .. summary(thread)
      end,
    }, function(thread)
      if thread then
        action(view, thread)
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
  end, "Next review thread")
  map("n", "[r", function()
    M.next_thread(-1)
  end, "Previous review thread")
  map("n", "<leader>rc", function()
    M.comment(vim.fn.line("."), vim.fn.line("."))
  end, "Comment on line")
  map("x", "<leader>rc", function()
    local start, finish = vim.fn.line("v"), vim.fn.line(".")
    vim.cmd("normal! \27")
    M.comment(start, finish)
  end, "Comment on selection")
  map("n", "<leader>rr", at_cursor(M.reply), "Reply to thread")
  map("n", "<leader>rs", at_cursor(M.set_status), "Set thread status")
  map("n", "<leader>rv", at_cursor(M.show_thread), "View thread")
end

-- Open review ID (default: this worktree's latest) on its newest revision.
function M.open(id, cwd)
  dots({ "show", id }, { cwd = cwd or checkout() }, function(out)
    local review = out.review
    local revision = review.revisions[#review.revisions]
    for tab, view in pairs(views) do
      if view.id == review.id and view.snapshot == revision.snapshot and vim.api.nvim_tabpage_is_valid(tab) then
        return vim.api.nvim_set_current_tabpage(tab)
      end
    end
    local root = review.worktree.path
    changed_files(root, revision.base, revision.snapshot, function(files)
      local tab, err = codediff.open({ root = root, base = revision.base, head = revision.snapshot, files = files })
      if not tab then
        return notify(err, vim.log.levels.ERROR)
      end
      local view = {
        id = review.id,
        tab = tab,
        cwd = root,
        file = out.file,
        revision = #review.revisions - 1,
        base = revision.base,
        snapshot = revision.snapshot,
        review = review,
        generation = 0,
      }
      views[tab] = view
      vim.fn.setqflist({}, " ", { title = "Review " .. review.id, context = { dots_review = review.id } })
      view.qf = vim.fn.getqflist({ id = 0 }).id
      watch(view)
      render_current()
    end)
  end)
end

function M.pick_reviews()
  dots({ "list", "--all" }, { cwd = checkout() }, function(out)
    if #out.reviews == 0 then
      return notify("no reviews in this repository")
    end
    table.sort(out.reviews, function(a, b)
      return a.updated_at > b.updated_at
    end)
    local items = {}
    for _, review in ipairs(out.reviews) do
      local worktree = review.worktree.branch or vim.fs.basename(review.worktree.path)
      local target = target_text(review.target)
      table.insert(items, {
        text = table.concat({ review.id, worktree, target, review.profile or "" }, " "),
        review = review,
        worktree = worktree,
        target = target,
      })
    end
    Snacks.picker.pick({
      title = "Reviews",
      items = items,
      layout = "select",
      format = function(item)
        local open = item.review.threads.open or 0
        return {
          { ("%-12s"):format(item.review.id), "Comment" },
          { ("%-24s"):format(item.worktree), "Directory" },
          { ("%-28s"):format(item.target) },
          { ("%3d open "):format(open), open > 0 and "DiagnosticWarn" or "Comment" },
          { ("%4s"):format(age(item.review.updated_at)), "Comment" },
        }
      end,
      confirm = function(picker, item)
        picker:close()
        if item then
          M.open(item.review.id, item.review.worktree.path)
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
    local where = location(view, thread)
    table.insert(items, {
      text = table.concat({ thread.id, thread.status, where, summary(thread) }, " "),
      thread = thread,
      where = where,
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
        { summary(item.thread) },
      }
    end,
    confirm = function(picker, item)
      picker:close()
      if not item then
        return
      end
      if item.thread.anchor and item.thread.revision == view.revision then
        jump_to(view, item.thread)
      else
        M.show_thread(view, item.thread)
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
  callback = render_current,
})

return M
