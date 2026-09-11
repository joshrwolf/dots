local M = {}

local variable = "herdr_review_binding_id"

local function valid(tabpage)
  return tabpage and vim.api.nvim_tabpage_is_valid(tabpage)
end

function M.get(tabpage)
  tabpage = tabpage or vim.api.nvim_get_current_tabpage()
  if not valid(tabpage) then
    return nil
  end
  local ok, value = pcall(vim.api.nvim_tabpage_get_var, tabpage, variable)
  return ok and type(value) == "string" and value ~= "" and value or nil
end

function M.set(tabpage, binding_id)
  if not valid(tabpage) then
    return
  end
  assert(type(binding_id) == "string" and binding_id ~= "", "review binding id must be a non-empty string")
  vim.api.nvim_tabpage_set_var(tabpage, variable, binding_id)
end

function M.clear(tabpage)
  if not valid(tabpage) then
    return
  end
  pcall(vim.api.nvim_tabpage_del_var, tabpage, variable)
end

function M.owns_request(request, binding_id)
  if not request or not binding_id then
    return false
  end
  for index = #(request.attempts or {}), 1, -1 do
    local attempt_binding = request.attempts[index].runtime_binding_id
    if attempt_binding then
      return attempt_binding == binding_id
    end
  end
  return request.runtime_binding_id == binding_id
end

return M
