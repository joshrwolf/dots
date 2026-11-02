-- Landing at a line of an explorer file, for codediff.api.show and for native
-- navigation (quickfix, location lists, :edit) into a tab's revision buffers.
local M = {}

local api = vim.api
local lifecycle = require("codediff.ui.lifecycle")
local virtual_file = require("codediff.core.virtual_file")

local SIDES = { "original", "modified" }
local pending = {} -- tabpage -> { path, side, line, callback } waiting for CodeDiffFileShown

local function explorer_session(tabpage)
  local session = tabpage and api.nvim_tabpage_is_valid(tabpage) and lifecycle.get_session(tabpage)
  if session and session.panel and session.panel.name == "explorer" and session.git_root then
    return session
  end
end

--- The revisions a tab compares: an explorer's, else a bare diff's.
---@return { root: string, base: string?, head: string? }?
function M.revisions(tabpage)
  local session = tabpage and api.nvim_tabpage_is_valid(tabpage) and lifecycle.get_session(tabpage)
  if not session then
    return nil
  end
  if session.panel and session.panel.name == "explorer" then
    local data = session.panel.data
    return { root = session.git_root, base = data.base_revision, head = data.target_revision }
  end
  return { root = session.git_root, base = session.original_revision, head = session.modified_revision }
end

--- Which side of `tabpage`'s comparison `buf` holds, if it is one of its
--- revision buffers.
---@return { path: string, revision: string, side: "original"|"modified" }?
function M.locate(tabpage, buf)
  local revisions = M.revisions(tabpage)
  if not revisions or not revisions.root or not api.nvim_buf_is_valid(buf) then
    return nil
  end
  local name = api.nvim_buf_get_name(buf)
  local _, revision, path = virtual_file.parse_url(name)
  if not path then
    return nil
  end
  -- Compare canonical URLs so the buffer must also belong to this repository.
  for _, side in ipairs(SIDES) do
    local wanted = side == "original" and revisions.base or revisions.head
    if wanted == revision and virtual_file.create_url(revisions.root, wanted, path) == name then
      return { path = path, revision = revision, side = side }
    end
  end
end

-- The explorer entry for path, shaped like the explorer's own selections.
local function find_file(session, path)
  local data = session.panel.data
  for _, group in ipairs({ "conflicts", "unstaged", "staged" }) do
    for _, file in ipairs((data.status_result or {})[group] or {}) do
      if file.path == path then
        return vim.tbl_extend("force", file, { group = group, git_root = data.git_root })
      end
    end
  end
end

local function land(tabpage)
  local target = pending[tabpage]
  pending[tabpage] = nil
  local session = explorer_session(tabpage)
  local win = session and session[target.side .. "_win"]
  if not win or not api.nvim_win_is_valid(win) then
    return
  end
  local buf = api.nvim_win_get_buf(win)
  api.nvim_win_set_cursor(win, { math.min(target.line, api.nvim_buf_line_count(buf)), 0 })
  require("codediff.ui.view.helpers").focus(win)
  if target.callback then
    target.callback(win, buf)
  end
end

--- Show `target.path` and put the cursor on `target.line` of `target.side`
--- once its content is loaded and its diff rendered. A later call for the same
--- tab replaces an earlier one that has not landed.
---@param tabpage integer
---@param target { path: string, side: "original"|"modified", line: integer }
---@param callback fun(win: integer, buf: integer)?
---@return boolean? ok, string? err
function M.show(tabpage, target, callback)
  local session = explorer_session(tabpage)
  if not session then
    return nil, "no CodeDiff explorer in tabpage " .. tostring(tabpage)
  end
  if target.side ~= "original" and target.side ~= "modified" then
    return nil, "side must be original or modified, not " .. tostring(target.side)
  end
  if type(target.line) ~= "number" or target.line < 1 then
    return nil, "line must be a positive number"
  end
  local file = find_file(session, target.path)
  if not file then
    return nil, tostring(target.path) .. " is not in this diff"
  end
  if target.side == "original" and (file.status == "A" or file.status == "??") then
    return nil, target.path .. " has no original side"
  elseif target.side == "modified" and file.status == "D" then
    return nil, target.path .. " has no modified side"
  elseif target.side == "original" and session.layout == "inline" then
    return nil, "the inline layout shows only the modified side"
  end

  pending[tabpage] = { path = target.path, side = target.side, line = math.floor(target.line), callback = callback }
  local refresh = require("codediff.ui.refresh")
  local selection = session.panel.data.current_selection
  local selected = selection and selection.path == target.path
  if selected and refresh.shown(tabpage) then
    land(tabpage)
  elseif not (selected and session.refresh and session.refresh.loading) then
    -- An unannounced selection that is not loading is redisplayed.
    refresh.select(tabpage, file, { no_jump = true, force = selected })
  end
  return true
end

-- The line a native jump asked for. A loaded buffer already has the cursor
-- on it; a buffer that was still loading clamped the cursor to its empty
-- placeholder, so read the current quickfix or location-list entry instead.
local function requested_line(win, buf, loaded)
  if loaded and api.nvim_win_is_valid(win) and api.nvim_win_get_buf(win) == buf then
    return api.nvim_win_get_cursor(win)[1]
  end
  local lists = { vim.fn.getqflist({ idx = 0, items = 0 }) }
  if api.nvim_win_is_valid(win) then
    table.insert(lists, 1, vim.fn.getloclist(win, { idx = 0, items = 0 }))
  end
  for _, list in ipairs(lists) do
    local item = list.items and list.items[1]
    if item and item.bufnr == buf and item.lnum > 0 then
      return item.lnum
    end
  end
  return 1
end

-- A revision buffer of an explorer tab entering one of its windows, other than
-- CodeDiff placing its own selection, is navigation: select that file and land
-- on the requested line. A window the jump opened (quickfix splits rather than
-- reuse 'nowrite' panes) is closed again.
local function follow(event)
  local win = api.nvim_get_current_win()
  -- Loading a buffer outside any window runs its autocommands in Neovim's
  -- hidden autocommand window; that is not navigation.
  if api.nvim_win_get_buf(win) ~= event.buf or vim.fn.win_gettype(win) == "autocmd" then
    return
  end
  local tabpage = api.nvim_win_get_tabpage(win)
  local session = explorer_session(tabpage)
  if not session or session.layout == "inline" or session.refresh and session.refresh.applying then
    return
  end
  local at = M.locate(tabpage, event.buf)
  if not at or not find_file(session, at.path) then
    return
  end
  local pane = win == session.original_win and "original" or win == session.modified_win and "modified" or nil
  local selection = session.panel.data.current_selection
  -- CodeDiff placing its selection, including a one-sided file reusing the
  -- other pane while the selection loads.
  local loading = session.refresh and session.refresh.loading
  if pane and selection and selection.path == at.path and (pane == at.side or loading) then
    return
  end
  local loaded = api.nvim_buf_is_loaded(event.buf) and not virtual_file.is_loading(event.buf)
  -- The jump may have wiped the buffer this window showed; a selection in
  -- progress keeps the session from being torn down for it.
  require("codediff.ui.refresh").begin(tabpage)
  vim.schedule(function()
    if not explorer_session(tabpage) then
      return
    end
    local line = requested_line(win, event.buf, loaded)
    local explorer = session.panel.view
    if not pane and api.nvim_win_is_valid(win) then
      if explorer and win == explorer.winid then
        if api.nvim_buf_is_valid(explorer.bufnr) then
          api.nvim_win_set_buf(win, explorer.bufnr)
        end
      elseif #api.nvim_tabpage_list_wins(tabpage) > 1 then
        api.nvim_win_close(win, true)
      end
    end
    require("codediff.ui.refresh").select(tabpage, find_file(session, at.path), { no_jump = true, force = true })
    M.show(tabpage, { path = at.path, side = at.side, line = line })
  end)
end

local group = api.nvim_create_augroup("CodeDiffNavigate", { clear = true })
api.nvim_create_autocmd("User", {
  group = group,
  pattern = "CodeDiffFileShown",
  callback = function(event)
    local tabpage = event.data.tabpage
    local target = pending[tabpage]
    if target and target.path == event.data.path then
      land(tabpage)
    elseif target then
      pending[tabpage] = nil -- Another file was shown since; the request is stale.
    end
  end,
})
api.nvim_create_autocmd("User", {
  group = group,
  pattern = "CodeDiffClose",
  callback = function(event)
    pending[event.data.tabpage] = nil
  end,
})
api.nvim_create_autocmd("BufWinEnter", {
  group = group,
  pattern = "codediff:///*",
  callback = follow,
})

return M
