-- `dots review` in CodeDiff: open a review on its exact revisions, show its
-- threads at their lines, and write comments, replies and statuses through the
-- CLI, which owns the review files. Agent changes appear live.
local M = {}

local author = "user"
local ns = vim.api.nvim_create_namespace("dots.review")
local group = vim.api.nvim_create_augroup("dots.review", { clear = true })
local views = {} -- tabpage -> open review view
local pending = {} -- "base..snapshot" -> view waiting for its CodeDiff tab

-- CodeDiff internals, checked against codediff.nvim v4.0.5. Its supported API
-- covers only hunk/file navigation and lifecycle events; everything else this
-- module needs from CodeDiff is here.
local codediff = {}

-- Side buffers are named codediff:///<root>///<commit>/<path>.
function codediff.parse(buf)
  return vim.api.nvim_buf_get_name(buf):match("^codediff:///.-///(%x+)/(.+)$")
end

function codediff.revisions(tab)
  local context = require("codediff.ui.lifecycle").get_git_context(tab)
  return context and context.original_revision, context and context.modified_revision
end

-- Whether buf's diff has rendered: CodeDiff clears the result while a file loads.
function codediff.rendered(tab, buf)
  local session = require("codediff.ui.lifecycle").get_session(tab)
  return session ~= nil
    and session.stored_diff_result ~= nil
    and (session.original_bufnr == buf or session.modified_bufnr == buf)
end

-- Show a changed path in the explorer without CodeDiff jumping to its first hunk.
function codediff.select(tab, path)
  local panel = require("codediff.ui.lifecycle").get_panel(tab)
  local explorer = panel and panel.name == "explorer" and panel.view
  local status = explorer and explorer.status_result or {}
  for _, name in ipairs({ "unstaged", "staged" }) do
    for _, file in ipairs(status[name] or {}) do
      if file.path == path then
        explorer.on_file_select(vim.tbl_extend("force", file, { group = name }), { no_jump = true })
        return true
      end
    end
  end
  return false
end

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
  local commit, path = codediff.parse(buf)
  local side = commit == view.base and "old" or commit == view.snapshot and "new" or nil
  return side and path, side
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

local function set_cursor(win, line)
  vim.api.nvim_set_current_win(win)
  vim.api.nvim_win_set_cursor(win, { math.min(line, vim.api.nvim_buf_line_count(0)), 0 })
  vim.cmd("normal! zz")
end

local setup_buffer

local function render(view, buf)
  local path, side = side_of(view, buf)
  if not side then
    return
  end
  setup_buffer(buf)
  vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  local count = vim.api.nvim_buf_line_count(buf)
  for _, thread in ipairs(anchored(view)) do
    local a = thread.anchor
    if a.path == path and a.side == side and a["end"] <= count then
      local icon, hl = style(thread)
      -- End-of-line text keeps both sides aligned; virtual lines would not.
      vim.api.nvim_buf_set_extmark(buf, ns, a.start - 1, 0, {
        sign_text = icon,
        sign_hl_group = hl,
        virt_text = { { "  " .. icon .. " " .. thread.status .. " ", hl }, { summary(thread), "Comment" } },
        virt_text_pos = "eol",
      })
      for line = a.start + 1, a["end"] do
        vim.api.nvim_buf_set_extmark(buf, ns, line - 1, 0, { sign_text = "│", sign_hl_group = hl })
      end
    end
  end
end

-- Reviews sharing a base share CodeDiff's side buffers, so notes are drawn for
-- the current tab's review only and redrawn when another tab is entered.
local function render_current()
  local tab = vim.api.nvim_get_current_tabpage()
  local view = views[tab]
  if view then
    for _, win in ipairs(vim.api.nvim_tabpage_list_wins(tab)) do
      render(view, vim.api.nvim_win_get_buf(win))
    end
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
      render_current()
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
end

local function jump_to(view, thread)
  local a = thread.anchor
  local function land()
    for _, win in ipairs(vim.api.nvim_tabpage_list_wins(view.tab)) do
      local buf = vim.api.nvim_win_get_buf(win)
      local path, side = side_of(view, buf)
      if path == a.path and side == a.side and codediff.rendered(view.tab, buf) then
        set_cursor(win, a.start)
        return true
      end
    end
  end
  if land() then
    return
  elseif not codediff.select(view.tab, a.path) then
    return notify(a.path .. " is not in this diff", vim.log.levels.WARN)
  end
  -- CodeDiff places both cursors once the new file's diff renders; land after.
  local deadline = vim.uv.now() + 5000
  local function retry()
    if views[view.tab] == view and not land() and vim.uv.now() < deadline then
      vim.defer_fn(retry, 20)
    end
  end
  vim.defer_fn(retry, 20)
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

-- A Markdown scratch float; submit hands its text to on_submit.
local function compose(title, on_submit, text)
  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].filetype = "markdown"
  vim.bo[buf].bufhidden = "wipe"
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, vim.split(text or "", "\n"))
  local win =
    open_float(buf, { width = 80, height = 8, title = " " .. title .. " ", footer = " <C-s> submit · q cancel " })
  local function submit()
    local body = vim.trim(table.concat(vim.api.nvim_buf_get_lines(buf, 0, -1, false), "\n"))
    vim.cmd.stopinsert()
    vim.api.nvim_win_close(win, true)
    if body == "" then
      return notify("empty comment discarded")
    end
    on_submit(body)
  end
  vim.keymap.set({ "n", "i" }, "<C-s>", submit, { buffer = buf, desc = "Submit comment" })
  vim.keymap.set("n", "q", function()
    vim.api.nvim_win_close(win, true)
  end, { buffer = buf, desc = "Cancel comment" })
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
    pending[revision.base .. ".." .. revision.snapshot] = {
      id = review.id,
      cwd = review.worktree.path,
      file = out.file,
      revision = #review.revisions - 1,
      base = revision.base,
      snapshot = revision.snapshot,
      review = review,
      generation = 0,
    }
    vim.api.nvim_cmd(
      { cmd = "CodeDiff", args = { "--repo", review.worktree.path, revision.base, revision.snapshot } },
      {}
    )
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
  pattern = "CodeDiffOpen",
  callback = function(event)
    local tab = event.data.tabpage
    local base, snapshot = codediff.revisions(tab)
    local view = base and pending[base .. ".." .. snapshot]
    if view then
      pending[base .. ".." .. snapshot] = nil
      view.tab = tab
      views[tab] = view
      watch(view)
      render_current()
    end
  end,
})

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

vim.api.nvim_create_autocmd("User", {
  group = group,
  pattern = "CodeDiffVirtualFileLoaded",
  callback = function(event)
    render_current()
  end,
})

-- Side buffers can finish loading before CodeDiff shows them.
vim.api.nvim_create_autocmd("BufWinEnter", {
  group = group,
  pattern = "codediff:///*",
  callback = function()
    render_current()
  end,
})

vim.api.nvim_create_autocmd("TabEnter", {
  group = group,
  callback = function()
    render_current()
  end,
})

return M
