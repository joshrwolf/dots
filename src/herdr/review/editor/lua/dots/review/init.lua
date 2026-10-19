-- Saved review discussions in ordinary Neovim. No editor controller, CodeDiff
-- internals, agent dispatch, or GitHub publication is required by this surface.
local client = require("dots.review.client")
local M = {}
local views = {}
local configured = false

local function notify(message, level)
  vim.notify("review: " .. message, level or vim.log.levels.WARN)
end

local function alive(buf, view)
  return vim.api.nvim_buf_is_valid(buf) and views[buf] == view
end

local function render(buf, thread)
  local lines = { "# Review thread · " .. thread.status, "" }
  local anchor = type(thread.anchor) == "table" and thread.anchor or nil
  if anchor then
    local original = anchor.original
    table.insert(
      lines,
      string.format("Anchor: %s:%d-%d (%s)", original.path, original.start_line, original.end_line, original.side)
    )
    table.insert(lines, "Saved anchor, not a claim about the current working-tree location.")
  else
    table.insert(lines, "General discussion")
  end
  if type(thread.finding) == "table" then
    vim.list_extend(lines, { "", "## " .. thread.finding.title, "", thread.finding.evidence })
  end
  for _, message in ipairs(thread.messages or {}) do
    vim.list_extend(lines, { "", "## " .. message.author, "" })
    vim.list_extend(lines, vim.split(message.body, "\n", { plain = true }))
  end
  vim.list_extend(lines, { "", ":ReviewReply · :ReviewResolve · :ReviewRefresh · q close" })
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, vim.split(table.concat(lines, "\n"), "\n", { plain = true }))
  vim.bo[buf].modified = false
  vim.bo[buf].modifiable = false
end

local function refresh(buf, view)
  view.busy = true
  client.load_binding(view.root, view.binding, function(state, err)
    if not alive(buf, view) then
      return
    end
    view.busy = false
    if err then
      notify(err.message, vim.log.levels.ERROR)
      return
    end
    for _, thread in ipairs(state.threads) do
      if thread.id == view.thread then
        view.uncertain = false
        render(buf, thread)
        return
      end
    end
    notify("thread is no longer present; reopen the thread list")
  end)
end

local function current()
  local buf = vim.api.nvim_get_current_buf()
  local view = views[buf]
  if not view then
    notify("open a discussion with :ReviewThreads first")
    return
  end
  if view.busy then
    notify("a review operation is still pending")
    return
  end
  return buf, view
end

local function changed(buf, view)
  return function(_, err)
    if not alive(buf, view) then
      return
    end
    view.busy = false
    if err then
      view.uncertain = true
      notify(
        err.message .. "; refresh and inspect before retrying. Reply draft is retained in this view.",
        vim.log.levels.ERROR
      )
      return
    end
    view.draft = nil
    refresh(buf, view)
  end
end

function M.reply()
  local buf, view = current()
  if not view then
    return
  end
  if view.uncertain then
    notify("refresh and inspect the discussion before another change")
    return
  end
  view.busy = true
  vim.ui.input({ prompt = "Review reply: ", default = view.draft or "" }, function(body)
    if not alive(buf, view) then
      return
    end
    if not body or not body:find("%S") then
      view.busy = false
      return
    end
    view.draft = body
    client.add_message(view.root, view.binding, view.thread, { body = body, author = "reviewer" }, changed(buf, view))
  end)
end

function M.resolve()
  local buf, view = current()
  if not view then
    return
  end
  if view.uncertain then
    notify("refresh and inspect the discussion before another change")
    return
  end
  view.busy = true
  client.resolve_thread(view.root, view.binding, view.thread, changed(buf, view))
end

function M.refresh()
  local buf, view = current()
  if view then
    refresh(buf, view)
  end
end

local function show(root, state, thread)
  vim.cmd("botright 12new")
  local buf = vim.api.nvim_get_current_buf()
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].swapfile = false
  vim.bo[buf].filetype = "markdown"
  vim.wo.wrap = true
  vim.wo.linebreak = true
  vim.api.nvim_buf_set_name(buf, "herdr-review://thread/" .. buf)
  views[buf] = { root = root, binding = state.binding.id, thread = thread.id, busy = false }
  render(buf, thread)
  vim.keymap.set("n", "q", "<cmd>close<cr>", { buffer = buf, desc = "Close review discussion" })
  vim.keymap.set("n", "<leader>rp", M.reply, { buffer = buf, desc = "Reply to review thread" })
  vim.keymap.set("n", "<leader>rr", M.resolve, { buffer = buf, desc = "Resolve review thread" })
  vim.api.nvim_create_autocmd("BufWipeout", {
    buffer = buf,
    once = true,
    callback = function()
      views[buf] = nil
    end,
  })
end

function M.threads()
  local origin = vim.api.nvim_get_current_buf()
  local tab = vim.api.nvim_get_current_tabpage()
  local existing = views[origin]
  local root = existing and existing.root or vim.fs.root(0, ".git") or vim.fn.getcwd()
  local function still_here()
    return vim.api.nvim_get_current_buf() == origin and vim.api.nvim_get_current_tabpage() == tab
  end
  client.load_context(root, function(context, err)
    if not still_here() then
      return
    end
    if err then
      notify(err.message, vim.log.levels.ERROR)
      return
    end
    if not context then
      notify("this checkout has no bound review; open one from Herdr first")
      return
    end
    if #context.state.threads == 0 then
      notify("this review has no saved threads")
      return
    end
    vim.ui.select(context.state.threads, {
      prompt = "Review threads (including resolved)",
      format_item = function(thread)
        local first = (thread.messages[1] or {}).body or thread.id
        return string.format("[%s] %s", thread.status, first:match("[^\n]+") or first)
      end,
    }, function(thread)
      if thread and still_here() then
        show(context.checkout_root, context.state, thread)
      end
    end)
  end)
end

function M.setup()
  if configured then
    return
  end
  configured = true
  vim.api.nvim_create_user_command("ReviewThreads", M.threads, {})
  vim.api.nvim_create_user_command("ReviewReply", M.reply, {})
  vim.api.nvim_create_user_command("ReviewResolve", M.resolve, {})
  vim.api.nvim_create_user_command("ReviewRefresh", M.refresh, {})
  vim.keymap.set("n", "<leader>fR", M.threads, { desc = "Find saved review threads" })
end

return M
