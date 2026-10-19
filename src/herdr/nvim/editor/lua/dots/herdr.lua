-- Composition root only. Production APIs stay fixed; tests construct the same
-- attachment/bridge with a narrow host fake rather than patching vim globals.
local Attachment = require("dots.herdr.attachment")
local Bridge = require("dots.herdr.bridge")
local Endpoint = require("dots.herdr.endpoint")
local P = require("dots.herdr.protocol")
local Host = require("dots.herdr.host")
local Listener = require("dots.herdr.listener")
local M = {}
local attachment, endpoint, dispatch

local function warn(err)
  vim.schedule(function()
    vim.notify("Herdr editor [" .. err.code .. "]: " .. err.message, vim.log.levels.WARN)
  end)
end

-- Transient $EDITOR callers should set DOTS_HERDR_EDITOR=0. A terminal UI by
-- itself cannot distinguish an intentional editor from a short-lived one.
function M.should_register()
  return vim.g.dots_herdr_editor ~= false
    and vim.env.DOTS_HERDR_EDITOR ~= "0"
    and (not vim.env.NVIM or vim.env.NVIM == "")
    and #vim.api.nvim_list_uis() > 0
end

function M.register()
  if
    not M.should_register()
    or vim.env.HERDR_ENV ~= "1"
    or not vim.env.HERDR_SOCKET_PATH
    or not vim.env.HERDR_PANE_ID
  then
    return false
  end
  if vim.env.HERDR_SOCKET_PATH:sub(1, 1) ~= "/" or vim.env.HERDR_SOCKET_PATH:find("%c") then
    warn({ code = "host_unavailable", message = "Herdr server must be an absolute socket path" })
    return false
  end
  if not attachment then
    local ok, result = pcall(function()
      return Endpoint.new(vim.env.HERDR_SOCKET_PATH, Listener.open())
    end)
    if not ok then
      warn(P.failure(result, "listener_unavailable"))
      return false
    end
    endpoint = result
    dispatch = Bridge.new(endpoint)
    attachment = Attachment.new(Host.new(vim.env.HERDR_SOCKET_PATH), endpoint, warn)
  end
  return attachment.start()
end

function M.request(json)
  if dispatch then
    return dispatch(json)
  end
  return vim.json.encode({
    status = "rejected",
    error = { code = "not_attached", message = "editor is not registered" },
  })
end

function M.setup()
  local group = vim.api.nvim_create_augroup("dots.herdr", { clear = true })
  vim.api.nvim_create_user_command(
    "HerdrEditorRegister",
    M.register,
    { desc = "Register this editor with its current Herdr pane", force = true }
  )
  if vim.v.vim_did_enter == 1 then
    M.register()
  else
    vim.api.nvim_create_autocmd("VimEnter", { group = group, once = true, callback = M.register })
  end
  vim.api.nvim_create_autocmd("VimLeavePre", {
    group = group,
    callback = function()
      -- This process owns the endpoint. Pane advertisements do not. Revoke
      -- local dispatch before potentially blocking on metadata cleanup.
      if endpoint then
        local ok, err = pcall(endpoint.close)
        if not ok then
          warn(P.failure(err, "listener_unavailable"))
        end
      end
      if attachment then
        attachment.stop()
      end
    end,
  })
end

return M
