local location = require("herdr_review.location")
local render = require("herdr_review.render")

local M = {}

local function first_line(value)
  return (value or ""):match("[^\n]+") or ""
end

local function latest_message(thread)
  local messages = thread.messages or {}
  return messages[#messages] or {}
end

local function preview(thread)
  local target = location.target(thread) or {}
  local anchor = type(thread.anchor) == "table" and thread.anchor or {}
  local context = anchor.context or {}
  local anchor_status = type(thread.resolution) == "table" and thread.resolution.status or "unavailable"
  local lines = {
    string.format("# %s · %s", location.label(thread), render.style(thread.status).label),
    "",
    target.path and string.format("Inline · `%s:%d-%d` · %s · anchor %s", target.path,
      target.start_line, target.end_line, target.side, anchor_status) or "General · review-level discussion",
  }
  local finding = location.finding(thread)
  if finding then
    vim.list_extend(lines, { "", "## " .. (finding.title or "Finding") })
    if type(finding.severity) == "string" then
      table.insert(lines, "Severity: " .. finding.severity)
    end
    if type(finding.evidence) == "string" and finding.evidence ~= "" then
      vim.list_extend(lines, { "", "### Evidence", "", finding.evidence })
    end
    for _, related in ipairs(finding.related_locations or {}) do
      table.insert(lines, string.format("Related: `%s:%d-%d` · %s", related.path,
        related.start_line, related.end_line, related.side))
    end
  end
  for _, message in ipairs(thread.messages or {}) do
    vim.list_extend(lines, { "", "## " .. (message.author or "reviewer"), "", message.body or "" })
  end
  local source = {}
  vim.list_extend(source, context.before or {})
  vim.list_extend(source, context.selected or {})
  vim.list_extend(source, context.after or {})
  if #source > 0 then
    vim.list_extend(lines, { "", "## Anchor context", "", "```" })
    vim.list_extend(lines, source)
    table.insert(lines, "```")
  end
  return table.concat(lines, "\n")
end

local function item_for(checkout_root, thread)
  local target = location.target(thread) or {}
  local style = render.style(thread.status)
  local anchor_status = type(thread.resolution) == "table" and thread.resolution.status
  local display_location = target.path and string.format("%s:%d", vim.fs.basename(target.path), location.line(target)) or "General"
  if anchor_status and anchor_status ~= "exact" then
    display_location = display_location .. " · " .. anchor_status
  end
  local message = latest_message(thread)
  return {
    text = table.concat({ style.label, location.label(thread), target.path or "General", display_location,
      (location.finding(thread) or {}).title or "", message.author or "", message.body or "" }, " "),
    file = target.path and vim.fs.joinpath(checkout_root, target.path) or nil,
    pos = target.path and { location.line(target), 0 } or nil,
    thread = thread,
    location = display_location,
    preview = { text = preview(thread), ft = "markdown", loc = false },
  }
end

local function format(item)
  local style = render.style(item.thread.status)
  return {
    { style.sign .. " ", style.sign_hl },
    { first_line((location.finding(item.thread) or {}).title or latest_message(item.thread).body), "SnacksPickerComment" },
    { " · " .. item.location, "SnacksPickerFile" },
  }
end

function M.show(thread)
  local bufnr = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, vim.split(preview(thread), "\n", { plain = true }))
  vim.bo[bufnr].filetype = "markdown"
  vim.bo[bufnr].modifiable = false
  vim.bo[bufnr].bufhidden = "wipe"
  local width = math.max(20, math.min(100, vim.o.columns - 4))
  local height = math.max(3, math.min(vim.api.nvim_buf_line_count(bufnr), vim.o.lines - 6))
  local win = vim.api.nvim_open_win(bufnr, true, {
    relative = "editor", width = width, height = height,
    row = math.floor((vim.o.lines - height) / 2), col = math.floor((vim.o.columns - width) / 2),
    border = "rounded", title = " Review thread ", style = "minimal",
  })
  vim.wo[win].wrap = true
  vim.wo[win].linebreak = true
  vim.wo[win].breakindent = true
  vim.keymap.set("n", "q", "<cmd>close<cr>", { buffer = bufnr, nowait = true })
  vim.keymap.set("n", "<Esc>", "<cmd>close<cr>", { buffer = bufnr, nowait = true })
end

function M.open(checkout_root, binding_state, actions)
  local items = {}
  for _, thread in ipairs(binding_state.threads or {}) do
    if thread.status == "open" then
      table.insert(items, item_for(checkout_root, thread))
    end
  end
  table.sort(items, function(left, right)
    return location.compare(left.thread, right.thread)
  end)
  if #items == 0 then
    vim.notify("review: there are no open review threads")
    return
  end
  Snacks.picker.pick({
    source = "review_threads",
    title = "Review Threads",
    items = items,
    format = format,
    preview = "preview",
    layout = "ivy",
    confirm = function(active_picker, item)
      active_picker:close()
      if item then
        vim.schedule(function()
          actions.jump(item.thread)
        end)
      end
    end,
    actions = {
      resolve_thread = function(active_picker, item)
        active_picker:close()
        if item then
          vim.schedule(function()
            actions.resolve(item.thread)
          end)
        end
      end,
    },
    win = {
      preview = { wo = { wrap = true, linebreak = true, breakindent = true } },
      input = { keys = { ["<c-r>"] = { "resolve_thread", mode = { "n", "i" }, desc = "Resolve review thread" } } },
      list = { keys = { r = { "resolve_thread", desc = "Resolve review thread" } } },
    },
  })
end

return M
