local M = {}

local observed = false
local generations = {}

local function lifecycle()
  return require("codediff.ui.lifecycle")
end

local function normalized(path)
  return vim.fs.normalize(vim.fn.fnamemodify(path, ":p"))
end

local function revision(value, missing_is_working)
  if value == "WORKING" or (value == nil and missing_is_working) then
    return { kind = "working_tree" }
  end
  if value == ":0" then
    return { kind = "index" }
  end
  if value and value:match("^:[1-3]$") then
    return nil, "merge-stage CodeDiff views cannot be reviewed"
  end
  if type(value) == "string" and value ~= "" then
    return { kind = "commit", oid = value }
  end
  return nil, "CodeDiff did not retain enough revision identity to anchor a thread"
end

local function validate_comparison(base, target)
  if base.kind == "working_tree" then
    return nil, "a mutable working tree cannot be the base side of a review comparison"
  end
  if target.kind == "index" and base.kind ~= "commit" then
    return nil, "only commit-to-index CodeDiff views can be reviewed"
  end
  if target.kind == "working_tree" and base.kind ~= "commit" and base.kind ~= "index" then
    return nil, "this CodeDiff working-tree comparison cannot be identified exactly"
  end
  if target.kind == "commit" and base.kind ~= "commit" then
    return nil, "only commit-to-commit revision ranges can be reviewed"
  end
  return { base = base, target = target }, nil
end

local function comparison_for(session)
  if session.merge then
    return nil, "merge CodeDiff views cannot be reviewed"
  end
  local original = session.original
  local modified = session.modified
  local base, base_err = revision(
    session.original_revision,
    original and original.relative and original.relative ~= ""
  )
  if not base then
    return nil, base_err
  end
  local target, target_err = revision(
    session.modified_revision,
    modified and modified.relative and modified.relative ~= ""
  )
  if not target then
    return nil, target_err
  end
  return validate_comparison(base, target)
end

local function resolve_head(checkout_root)
  local result = vim.system({ "git", "rev-parse", "--verify", "HEAD^{commit}" }, {
    cwd = checkout_root,
    text = true,
  }):wait(3000)
  local oid = result and vim.trim(result.stdout or "") or ""
  if not result or result.code ~= 0 or not oid:match("^[0-9a-fA-F]+$") then
    return nil, "CodeDiff's implicit HEAD could not be resolved to an exact commit"
  end
  return { kind = "commit", oid = oid:lower() }, nil
end

local function exact_revision(value, missing_is_working, checkout_root)
  if value == "HEAD" then
    return resolve_head(checkout_root)
  end
  return revision(value, missing_is_working)
end

local function contains_path(files, selected_path)
  for _, file in ipairs(files or {}) do
    if file.path == selected_path then
      return true
    end
  end
  return false
end

local function selected_path_matches(panel, original, modified)
  local selected = panel.current_file_path
  if type(selected) ~= "string" or selected == "" then
    return false
  end
  local old_path = panel.current_selection and panel.current_selection.old_path
  return (original and (original.relative == selected or original.relative == old_path))
    or (modified and (modified.relative == selected or modified.relative == old_path))
end

local function explorer_comparison(code_diff, tabpage, session, original, modified)
  if code_diff.get_panel_name(tabpage) ~= "explorer" then
    return nil, "single-sided files require a CodeDiff explorer comparison"
  end
  local panel = code_diff.get_panel_view(tabpage)
  if not panel or not selected_path_matches(panel, original, modified) then
    return nil, "CodeDiff is still committing the selected file comparison"
  end
  if panel.current_file_group == "conflicts" then
    return nil, "merge CodeDiff views cannot be reviewed"
  end

  local base_value = session.original_revision or panel.base_revision
  if base_value == "HEAD" and panel.base_revision then
    base_value = panel.base_revision
  end
  local target_value = session.modified_revision or panel.target_revision
  if base_value ~= nil or target_value ~= nil then
    local base, base_err = exact_revision(base_value, false, session.git_root)
    if not base and target_value == ":0" then
      base, base_err = resolve_head(session.git_root)
    end
    if not base then
      return nil, base_err
    end
    local target, target_err = exact_revision(
      target_value,
      panel.current_file_group == "unstaged" and target_value == nil,
      session.git_root
    )
    if not target and panel.current_file_group == "staged" and target_value == nil then
      target, target_err = { kind = "index" }, nil
    end
    if not target then
      return nil, target_err
    end
    return validate_comparison(base, target)
  end

  local group = panel.current_file_group
  if group ~= "staged" and group ~= "unstaged" then
    return nil, "CodeDiff has not selected a reviewable staged or unstaged file"
  end
  if group == "staged" then
    local head, head_err = resolve_head(session.git_root)
    if not head then
      return nil, head_err
    end
    return validate_comparison(head, { kind = "index" })
  end
  if contains_path(panel.status_result and panel.status_result.staged, panel.current_file_path) then
    return validate_comparison({ kind = "index" }, { kind = "working_tree" })
  end
  local head, head_err = resolve_head(session.git_root)
  if not head then
    return nil, head_err
  end
  return validate_comparison(head, { kind = "working_tree" })
end

function M.view(tabpage)
  tabpage = tabpage or vim.api.nvim_get_current_tabpage()
  local code_diff = lifecycle()
  local session = code_diff.get_session(tabpage)
  if not session then
    return nil, "the CodeDiff tab has already closed"
  end
  if not session.git_root then
    return nil, "this CodeDiff session is not backed by a Git repository"
  end
  if session.stored_diff_result == nil then
    return nil, "CodeDiff is still updating the selected comparison"
  end
  local original_bufnr, modified_bufnr = code_diff.get_buffers(tabpage)
  local original, modified = code_diff.get_paths(tabpage)
  local comparison, comparison_err = comparison_for(session)
  if not comparison and session.single_side then
    comparison, comparison_err = explorer_comparison(code_diff, tabpage, session, original, modified)
  end
  if not comparison then
    return nil, comparison_err
  end
  return {
    checkout_root = normalized(session.git_root),
    tabpage = tabpage,
    generation = generations[tabpage] or 0,
    comparison = comparison,
    sides = {
      base = { bufnr = original_bufnr, path = original and original.relative },
      target = { bufnr = modified_bufnr, path = modified and modified.relative },
    },
  }, nil
end

function M.same_view(left, right)
  return left ~= nil and right ~= nil
    and left.tabpage == right.tabpage
    and left.generation == right.generation
    and vim.deep_equal(left.comparison, right.comparison)
end

function M.view_for_buffer(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  local code_diff = lifecycle()
  local tabpage = code_diff.find_tabpage_by_buffer(bufnr)
  if not tabpage then
    return nil, "the current buffer is not a CodeDiff side"
  end
  local view, err = M.view(tabpage)
  if not view then
    return nil, err
  end
  local original_bufnr, modified_bufnr = code_diff.get_buffers(tabpage)
  local side = bufnr == original_bufnr and "base" or bufnr == modified_bufnr and "target" or nil
  if not side then
    return nil, "threads are only supported on CodeDiff's base and target sides"
  end
  local path = view.sides[side].path
  if not path or path == "" then
    return nil, "select a file in CodeDiff before adding a thread"
  end
  view.bufnr = bufnr
  view.side = side
  view.path = path
  return view, nil
end

function M.capture(line1, line2, bufnr)
  local view, err = M.view_for_buffer(bufnr)
  if not view then
    return nil, err
  end
  local count = vim.api.nvim_buf_line_count(view.bufnr)
  line1 = math.max(1, math.min(line1 or vim.api.nvim_win_get_cursor(0)[1], count))
  line2 = math.max(1, math.min(line2 or line1, count))
  if line1 > line2 then
    line1, line2 = line2, line1
  end
  local target = { kind = line1 == line2 and "line" or "range", path = view.path, side = view.side }
  if line1 == line2 then
    target.line = line1
  else
    target.start_line = line1
    target.end_line = line2
  end
  view.start_line = line1
  view.end_line = line2
  view.target = target
  view.anchor_context = {
    before = vim.api.nvim_buf_get_lines(view.bufnr, math.max(0, line1 - 4), line1 - 1, false),
    selected = vim.api.nvim_buf_get_lines(view.bufnr, line1 - 1, line2, false),
    after = vim.api.nvim_buf_get_lines(view.bufnr, line2, math.min(count, line2 + 3), false),
  }
  return view, nil
end

function M.tab_for_buffer(bufnr)
  return lifecycle().find_tabpage_by_buffer(bufnr or vim.api.nvim_get_current_buf())
end

local function endpoint_matches(captured, current)
  if not captured or not current then
    return false
  end
  if current.kind ~= "commit" then
    return false
  end
  return captured.oid ~= nil and current.oid == captured.oid
end

function M.matches_observation(comparison, observation)
  local captured = observation and observation.comparison
  return comparison ~= nil and captured ~= nil
    and endpoint_matches(captured.base, comparison.base)
    and endpoint_matches(captured.target, comparison.target)
end

local function focus_target(tabpage, target, line)
  local view = M.view(tabpage)
  local side = view and view.sides[target.side]
  if not side or side.path ~= target.path or not side.bufnr or not vim.api.nvim_buf_is_valid(side.bufnr) then
    return false
  end
  local original_win, modified_win = lifecycle().get_windows(tabpage)
  local win = target.side == "base" and original_win or modified_win
  if not win or not vim.api.nvim_win_is_valid(win) then
    return false
  end
  vim.api.nvim_set_current_tabpage(tabpage)
  vim.api.nvim_set_current_win(win)
  line = math.min(line or 1, vim.api.nvim_buf_line_count(side.bufnr))
  vim.api.nvim_win_set_cursor(win, { math.max(1, line), 0 })
  vim.cmd.normal({ "zz", bang = true })
  return true
end

local function explorer_file(panel, target, comparison)
  local preferred = comparison.target.kind == "index" and "staged" or "unstaged"
  for _, group in ipairs({ preferred, preferred == "staged" and "unstaged" or "staged", "conflicts" }) do
    for _, file in ipairs((panel.status_result and panel.status_result[group]) or {}) do
      if file.path == target.path or file.old_path == target.path then
        local selected = vim.deepcopy(file)
        selected.group = selected.group or group
        return selected
      end
    end
  end
end

function M.jump(tabpage, target, line, callback)
  callback = callback or function() end
  if focus_target(tabpage, target, line) then
    callback(nil)
    return
  end
  local view, err = M.view(tabpage)
  local panel = view and lifecycle().get_panel_view(tabpage)
  local file = panel and explorer_file(panel, target, view.comparison)
  if not view or not panel or type(panel.on_file_select) ~= "function" or not file then
    callback(err or "the thread target is not available in this CodeDiff comparison")
    return
  end
  panel.on_file_select(file)
  local deadline = (vim.uv or vim.loop).now() + 3000
  local function finish_when_ready()
    if focus_target(tabpage, target, line) then
      callback(nil)
    elseif (vim.uv or vim.loop).now() >= deadline then
      callback("CodeDiff did not finish selecting the thread target")
    else
      vim.defer_fn(finish_when_ready, 20)
    end
  end
  vim.defer_fn(finish_when_ready, 20)
end

local function diff_endpoint(endpoint)
  if endpoint.source and endpoint.oid then
    return { kind = "commit", oid = endpoint.oid }
  end
  return endpoint
end

function M.open(comparison)
  local base = diff_endpoint(comparison.base)
  local target = diff_endpoint(comparison.target)
  local args
  if base.kind == "commit" and target.kind == "commit" then
    args = { base.oid, target.oid }
  elseif base.kind == "commit" and target.kind == "working_tree" then
    args = { base.oid }
  elseif base.kind == "commit" and target.kind == "index" then
    args = { "--staged", base.oid }
  elseif base.kind == "index" and target.kind == "working_tree" then
    args = {}
  else
    return nil, "the binding contains an unsupported comparison"
  end
  vim.api.nvim_cmd({ cmd = "CodeDiff", args = args }, {})
  return true
end

function M.bind_keymaps(tabpage, handlers)
  local code_diff = lifecycle()
  local view = M.view(tabpage)
  if not view then
    M.release_keymaps(tabpage)
    return
  end
  code_diff.begin_keymap_scope(tabpage, "herdr-review")
  for _, side in pairs(view.sides) do
    local bufnr = side.bufnr
    if bufnr and vim.api.nvim_buf_is_valid(bufnr) and side.path and side.path ~= "" then
      local meta = { priority = 100 }
      for _, mode in ipairs({ "n", "x" }) do
        code_diff.set_buf_keymap(tabpage, bufnr, mode, "<leader>r", function()
          require("which-key").show({ keys = "<leader>r", mode = mode })
        end, { desc = "Review", nowait = false }, meta)
      end
      code_diff.set_buf_keymap(tabpage, bufnr, "n", "<leader>ra", handlers.create_thread, { desc = "Add review thread" }, meta)
      code_diff.set_buf_keymap(tabpage, bufnr, "x", "<leader>ra", handlers.create_range_thread, { desc = "Add review thread" }, meta)
      code_diff.set_buf_keymap(tabpage, bufnr, "n", "<leader>rr", handlers.resolve_thread, { desc = "Resolve review thread" }, meta)
      code_diff.set_buf_keymap(tabpage, bufnr, "n", "<leader>rp", handlers.reply_thread, { desc = "Reply to review thread" }, meta)
      code_diff.set_buf_keymap(tabpage, bufnr, "n", "<leader>rR", handlers.refresh, { desc = "Refresh review activity" }, meta)
      code_diff.set_buf_keymap(tabpage, bufnr, "n", "<leader>rs", handlers.status, { desc = "Review activity status" }, meta)
      code_diff.set_buf_keymap(tabpage, bufnr, "n", "<leader>rS", handlers.request_agent, { desc = "Send review messages" }, meta)
      code_diff.set_buf_keymap(tabpage, bufnr, "n", "[r", handlers.previous_thread, { desc = "Previous review thread" }, meta)
      code_diff.set_buf_keymap(tabpage, bufnr, "n", "]r", handlers.next_thread, { desc = "Next review thread" }, meta)
    end
  end
  code_diff.end_keymap_scope(tabpage, "herdr-review")
end

function M.release_keymaps(tabpage)
  local ok, code_diff = pcall(lifecycle)
  if ok then
    code_diff.release_keymap_scope(tabpage, "herdr-review")
  end
end

function M.refresh_explorer(tabpage)
  local panel = lifecycle().get_panel_view(tabpage)
  if panel and panel.tree and type(panel.tree.render) == "function" then
    panel.tree:render()
  end
end

function M.observe(callbacks)
  if observed then
    return
  end
  observed = true
  local code_diff = lifecycle()
  local function ready(tabpage)
    vim.schedule(function()
      if code_diff.get_session(tabpage) then
        callbacks.ready(tabpage)
      end
    end)
  end
  local create_session = code_diff.create_session
  code_diff.create_session = function(tabpage, session_config, panes)
    local created = create_session(tabpage, session_config, panes)
    generations[tabpage] = (generations[tabpage] or 0) + 1
    ready(tabpage)
    return created
  end
  local update_diff_result = code_diff.update_diff_result
  code_diff.update_diff_result = function(tabpage, diff_result)
    local updated = update_diff_result(tabpage, diff_result)
    if updated then
      generations[tabpage] = (generations[tabpage] or 0) + 1
      if diff_result == nil then
        callbacks.updating(tabpage)
      else
        ready(tabpage)
      end
    end
    return updated
  end
end

function M.configure(options)
  local formatters = require("codediff.ui.explorer.formatters")
  options.explorer = options.explorer or {}
  options.explorer.formatters = options.explorer.formatters or {}
  options.explorer.formatters.file = function(context)
    local line = formatters.file(context)
    local count = require("herdr_review.render").count_for_path(context.path)
    if count > 0 then
      table.insert(line.right, 1, {
        segments = { { text = "◆" .. count .. " ", hl = "DiagnosticSignWarn" } },
      })
    end
    return line
  end
  require("codediff").setup(options)

  local layout = require("codediff.ui.layout")
  local config = require("codediff.config")
  local arrange = layout.arrange
  layout.arrange = function(tabpage)
    local explorer = config.options.explorer
    if not (explorer and explorer.position == "right") then
      return arrange(tabpage)
    end
    explorer.position = "left"
    local ok, err = pcall(arrange, tabpage)
    explorer.position = "right"
    if not ok then
      error(err)
    end
  end
end

return M
