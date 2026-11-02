-- Public API for callers that build on CodeDiff views, such as a review layer.
-- It knows diffs, files and lines, never what a caller layers on them.
-- Documented in README.md ("Lua API"); covered by tests/integration/plugin/lua_api_spec.lua.
local M = {}

local navigate = require("codediff.ui.explorer.navigate")
local lifecycle = require("codediff.ui.lifecycle")
local path = require("codediff.core.path")
local virtual_file = require("codediff.core.virtual_file")

local STATUSES = { A = true, M = true, D = true }

--- Open an explorer tab comparing two exact commits over a caller-supplied
--- file set. The list stays as given: Git status never replaces it.
---@param opts { root: string, base: string, head: string, files: { path: string, status: "A"|"M"|"D" }[] }
---@return integer? tabpage, string? err
function M.open(opts)
  if type(opts) ~= "table" then
    return nil, "open needs a table"
  end
  if type(opts.root) ~= "string" or vim.fn.isdirectory(opts.root) == 0 then
    return nil, "root must be a repository directory, not " .. tostring(opts.root)
  end
  for _, key in ipairs({ "base", "head" }) do
    if type(opts[key]) ~= "string" or not opts[key]:match("^%x+$") or #opts[key] < 40 then
      return nil, key .. " must be a full commit id, not " .. tostring(opts[key])
    end
  end
  if type(opts.files) ~= "table" or #opts.files == 0 then
    return nil, "files must list at least one file"
  end
  local files = {}
  for i, file in ipairs(opts.files) do
    if type(file.path) ~= "string" or file.path == "" or not STATUSES[file.status] then
      return nil, ("files[%d] needs a path and a status of A, M or D"):format(i)
    end
    files[i] = { path = file.path, status = file.status }
  end

  local root = vim.fn.fnamemodify(opts.root, ":p"):gsub("/$", "")
  require("codediff.ui.view").create({
    panel = {
      name = "explorer",
      data = { status_result = { unstaged = files, staged = {}, conflicts = {} }, fixed_files = true },
    },
    git_root = root,
    original = path.empty(),
    modified = path.empty(),
    original_revision = opts.base,
    modified_revision = opts.head,
  }, "")
  return vim.api.nvim_get_current_tabpage()
end

--- The repository root and revisions `tabpage` compares; base or head is nil
--- for a side that is not a revision (the working tree, a status view).
---@param tabpage integer?
---@return { root: string, base: string?, head: string? }?
function M.revisions(tabpage)
  return navigate.revisions(tabpage or vim.api.nvim_get_current_tabpage())
end

--- The file and side `buf` shows in `tabpage`'s comparison, or nil when it is
--- not one of that tab's revision buffers.
---@param buf integer
---@param tabpage integer?
---@return { path: string, revision: string, side: "original"|"modified" }?
function M.buffer(buf, tabpage)
  return navigate.locate(tabpage or vim.api.nvim_get_current_tabpage(), buf)
end

--- The revision buffer for one side of a file in `tabpage`'s comparison,
--- created unloaded when it does not exist yet. CodeDiff loads it when shown.
---@param tabpage integer
---@param file_path string
---@param side "original"|"modified"
---@return integer? buf, string? err
function M.revision_buffer(tabpage, file_path, side)
  local revisions = M.revisions(tabpage)
  local revision = revisions and (side == "original" and revisions.base or side == "modified" and revisions.head or nil)
  if not revision or revision == "WORKING" or not revisions.root then
    return nil, "tabpage " .. tostring(tabpage) .. " has no " .. tostring(side) .. " revision"
  end
  return vim.fn.bufadd(virtual_file.create_url(revisions.root, revision, file_path))
end

--- Show a file of an explorer tab and put the cursor on a line of one side
--- once its content has loaded and its diff has rendered; then call
--- `callback(win, buf)`. A newer call for the same tab supersedes a pending one.
---@param tabpage integer
---@param target { path: string, side: "original"|"modified", line: integer }
---@param callback fun(win: integer, buf: integer)?
---@return boolean? ok, string? err
function M.show(tabpage, target, callback)
  return navigate.show(tabpage, target, callback)
end

--- Replace the explorer's per-file decorations: a map from path to
--- `{ text, hl }`, drawn on that file's row. An empty map clears them.
---@param tabpage integer
---@param decorations table<string, { text: string, hl: string? }>
---@return boolean? ok, string? err
function M.set_decorations(tabpage, decorations)
  local panel = vim.api.nvim_tabpage_is_valid(tabpage) and lifecycle.get_panel(tabpage)
  if not panel or panel.name ~= "explorer" then
    return nil, "no CodeDiff explorer in tabpage " .. tostring(tabpage)
  end
  panel.decorations = vim.deepcopy(decorations or {})
  local explorer = panel.view
  if explorer and not explorer.is_hidden and vim.api.nvim_buf_is_valid(explorer.bufnr) then
    explorer.tree:render()
  end
  return true
end

return M
