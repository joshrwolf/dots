local binding = require("herdr_review.binding")
local codediff = require("herdr_review.codediff")
local location = require("herdr_review.location")
local wrap = require("herdr_review.wrap")

local M = {}
local tabs = {}
local formatting_tabpage

local styles = {
  open = {
    sign = "◆", label = "open", sign_hl = "DiagnosticSignWarn",
    text_hl = "DiagnosticVirtualTextWarn", anchor_hl = "DiagnosticUnderlineWarn",
  },
  resolved = {
    sign = "✓", label = "resolved", sign_hl = "DiagnosticSignOk",
    text_hl = "DiagnosticVirtualTextOk", anchor_hl = "DiagnosticUnderlineOk",
  },
}
local stale_style = {
  sign = "!", label = "stale", sign_hl = "DiagnosticSignError",
  text_hl = "DiagnosticVirtualTextError", anchor_hl = "DiagnosticUnderlineError",
}

function M.style(status)
  return styles[status] or styles.open
end

local function state_for(tabpage)
  if not tabs[tabpage] then
    tabs[tabpage] = {
      namespace = vim.api.nvim_create_namespace("herdr-review:" .. tabpage),
      buffers = {}, anchors = {}, groups = {},
    }
  end
  return tabs[tabpage]
end

local function clear_decorations(state)
  for bufnr in pairs(state.buffers) do
    if vim.api.nvim_buf_is_valid(bufnr) then
      vim.api.nvim_buf_clear_namespace(bufnr, state.namespace, 0, -1)
    end
  end
  state.buffers = {}
end

local function clear(tabpage)
  local state = tabs[tabpage]
  if state then
    clear_decorations(state)
    state.anchors = {}
    state.groups = {}
  end
end

local function anchor_status(thread)
  return (thread.resolution and thread.resolution.status) or "unavailable"
end

local function is_stale(thread)
  local status = anchor_status(thread)
  return status == "modified" or status == "ambiguous" or status == "deleted" or status == "unavailable"
end

local function group_style(group)
  for _, entry in ipairs(group.threads) do
    if entry.stale then
      return stale_style
    end
  end
  return M.style(group.threads[1].thread.status)
end

local function thread_lines(group, width)
  local lines = {}
  for index, entry in ipairs(group.threads) do
    local thread = entry.thread
    local style = entry.stale and stale_style or M.style(thread.status)
    table.insert(lines, { {
      string.format("%s%s %s · %s", index == 1 and "  ╭─ " or "  ├─ ", style.sign, style.label, location.label(thread)),
      style.text_hl,
    } })
    local finding = location.finding(thread)
    if finding then
      table.insert(lines, { { "  │  " .. (finding.title or "Finding"), style.text_hl } })
    end
    for _, message in ipairs(thread.messages or {}) do
      table.insert(lines, { { "  │  " .. (message.author or "reviewer") .. " ·", style.text_hl } })
      for _, line in ipairs(vim.split(message.body or "", "\n", { plain = true })) do
        table.insert(lines, { { "  │  " .. line, "NormalFloat" } })
      end
    end
    if entry.stale then
      table.insert(lines, { { "  │  anchor " .. entry.anchor_status, stale_style.text_hl } })
    end
  end
  table.insert(lines, { { "  ╰─", group_style(group).text_hl } })
  local wrapped = {}
  for _, chunks in ipairs(lines) do
    local text, highlight = chunks[1][1], chunks[1][2]
    local prefix = vim.fn.strcharpart(text, 0, 5)
    local content = vim.fn.strcharpart(text, 5)
    for index, part in ipairs(wrap.lines(content, width - vim.fn.strdisplaywidth(prefix))) do
      wrapped[#wrapped + 1] = { { (index == 1 and prefix or "  │  ") .. part, highlight } }
    end
  end
  return wrapped
end

local function render_prose(state, group)
  local width
  for _, win in ipairs(vim.api.nvim_tabpage_list_wins(state.tabpage)) do
    if vim.api.nvim_win_get_buf(win) == group.bufnr then
      local info = vim.fn.getwininfo(win)[1]
      local available = vim.api.nvim_win_get_width(win) - (info and info.textoff or 0) - 1
      width = math.min(width or available, available)
    end
  end
  group.prose_id = vim.api.nvim_buf_set_extmark(group.bufnr, state.namespace, group.start_line - 1, 0, {
    id = group.prose_id, virt_lines = thread_lines(group, width or 80),
    virt_lines_above = true, priority = 90, strict = false,
  })
end

local function render_group(state, group)
  local style = group_style(group)
  group.mark_id = vim.api.nvim_buf_set_extmark(group.bufnr, state.namespace, group.start_line - 1, 0, {
    end_row = group.end_line, end_col = 0, hl_group = style.anchor_hl,
    priority = 90, strict = false, right_gravity = true,
    end_right_gravity = false, undo_restore = true, invalidate = true,
  })
  for line = group.start_line, group.end_line do
    vim.api.nvim_buf_set_extmark(group.bufnr, state.namespace, line - 1, 0, {
      sign_text = "▌", sign_hl_group = style.sign_hl,
      number_hl_group = style.sign_hl, priority = 90, strict = false,
    })
  end
  render_prose(state, group)
  state.buffers[group.bufnr] = true
end

local function sync_live_ranges(state)
  for bufnr, groups in pairs(state.groups) do
    if vim.api.nvim_buf_is_valid(bufnr) then
      for _, group in ipairs(groups) do
        if group.mark_id then
          local mark = vim.api.nvim_buf_get_extmark_by_id(bufnr, state.namespace, group.mark_id, { details = true })
          local details = mark[3]
          if #mark > 0 and details and not details.invalid and details.end_row then
            group.start_line = mark[1] + 1
            group.end_line = math.max(group.start_line, details.end_row)
            for _, entry in ipairs(group.threads) do
              entry.start_line, entry.end_line = group.start_line, group.end_line
            end
          end
        end
      end
    end
  end
end

-- Resizing changes presentation only: retain live anchors and their edit history.
function M.reflow()
  for tabpage, state in pairs(tabs) do
    if vim.api.nvim_tabpage_is_valid(tabpage) then
      sync_live_ranges(state)
      for bufnr, groups in pairs(state.groups) do
        if vim.api.nvim_buf_is_valid(bufnr) then
          for _, group in ipairs(groups) do
            render_prose(state, group)
          end
        end
      end
    end
  end
end

local function classify(view, thread)
  local target = location.target(thread)
  local side = target and view.sides[target.side]
  local start_line, end_line = location.lines(target)
  if not side or not target.path or not start_line or target.path ~= side.path then
    return nil
  end
  local bufnr = side.bufnr
  if not bufnr or not vim.api.nvim_buf_is_valid(bufnr) then
    return nil
  end
  local count = vim.api.nvim_buf_line_count(bufnr)
  if start_line < 1 or end_line < start_line or end_line > count then
    return nil
  end
  return {
    bufnr = bufnr, start_line = start_line, end_line = end_line,
    anchor_status = anchor_status(thread), stale = is_stale(thread), thread = thread,
  }
end

local function add_entry(state, entry)
  local key = string.format("%d:%d:%d", entry.bufnr, entry.start_line, entry.end_line)
  local groups = state.groups[entry.bufnr] or {}
  state.groups[entry.bufnr] = groups
  local group = vim.iter(groups):find(function(candidate)
    return candidate.key == key
  end)
  if not group then
    group = {
      key = key, bufnr = entry.bufnr, start_line = entry.start_line,
      end_line = entry.end_line, threads = {},
    }
    table.insert(groups, group)
  end
  table.insert(group.threads, entry)
  state.anchors[entry.bufnr] = state.anchors[entry.bufnr] or {}
  table.insert(state.anchors[entry.bufnr], entry)
end

function M.request_summary(binding_state, binding_id)
  local requests = binding_state and binding_state.agent_requests or {}
  local request = requests[#requests]
  if not request then
    return nil
  end
  local attempts = request.attempts or {}
  local attempt = attempts[#attempts]
  local owner = attempt and attempt.runtime_binding_id or request.runtime_binding_id
  local ownership = owner and owner ~= binding_id and " · another runtime" or ""
  if not attempt then
    return string.format("request %s · %s%s", request.ordinal or "?", request.state or "unknown", ownership)
  end
  local assignment = attempt.assignment or {}
  local agent = assignment.expected_agent_name or assignment.pane_id or "unassigned agent"
  local detail = attempt.detail and (" · " .. attempt.detail:gsub("\n", " ")) or ""
  return string.format("request %s · %s · %s%s%s", request.ordinal or "?", attempt.state or request.state or "unknown", agent, ownership, detail)
end

function M.refresh(tabpage, view, binding_state, coherent)
  local state = state_for(tabpage)
  state.tabpage = tabpage
  state.checkout_root = view.checkout_root
  local live = {}
  if binding_state and binding_state.observation and state.binding_state and state.binding_state.observation
    and binding_state.observation.id == state.binding_state.observation.id then
    sync_live_ranges(state)
    for _, entries in pairs(state.anchors) do
      for _, entry in ipairs(entries) do live[entry.thread.id] = entry end
    end
  end
  clear(tabpage)
  if binding_state and (coherent or codediff.matches_observation(view.comparison, binding_state.observation)) then
    state.binding_state = binding_state
    for _, thread in ipairs(binding_state.threads or {}) do
      local entry = classify(view, thread)
      if entry then
        local previous = live[thread.id]
        if previous and previous.bufnr == entry.bufnr
          and vim.deep_equal(previous.thread.resolution, thread.resolution)
          and vim.deep_equal(location.target(previous.thread), location.target(thread)) then
          entry.start_line, entry.end_line = previous.start_line, previous.end_line
        end
        add_entry(state, entry)
      end
    end
    sync_live_ranges(state)
    clear_decorations(state)
    for _, groups in pairs(state.groups) do
      for _, group in ipairs(groups) do
        render_group(state, group)
      end
    end
  else
    state.binding_state = nil
  end
  formatting_tabpage = tabpage
  local ok, err = pcall(codediff.refresh_explorer, tabpage)
  formatting_tabpage = nil
  if not ok then
    error(err)
  end
end

function M.count_for_path(path, tabpage)
  local state = tabs[tabpage or formatting_tabpage or vim.api.nvim_get_current_tabpage()]
  local count = 0
  for _, thread in ipairs((state and state.binding_state and state.binding_state.threads) or {}) do
    local target = location.target(thread)
    if thread.status == "open" and target and target.path == path then
      count = count + 1
    end
  end
  return count
end

function M.threads_at_cursor()
  local state = tabs[vim.api.nvim_get_current_tabpage()]
  if not state then
    return {}
  end
  local bufnr = vim.api.nvim_get_current_buf()
  local line = vim.api.nvim_win_get_cursor(0)[1]
  sync_live_ranges(state)
  local threads = {}
  for _, entry in ipairs(state.anchors[bufnr] or {}) do
    if line >= entry.start_line and line <= entry.end_line then
      table.insert(threads, entry.thread)
    end
  end
  return threads
end

function M.location(thread, tabpage)
  local state = tabs[tabpage or vim.api.nvim_get_current_tabpage()]
  if state then
    for _, entries in pairs(state.anchors) do
      for _, entry in ipairs(entries) do
        if entry.thread.id == thread.id then
          sync_live_ranges(state)
          local target = vim.deepcopy(location.target(thread) or {})
          target.start_line, target.end_line = entry.start_line, entry.end_line
          return target
        end
      end
    end
  end
  return location.target(thread)
end

function M.detach(tabpage)
  clear(tabpage)
  tabs[tabpage] = nil
end

return M
