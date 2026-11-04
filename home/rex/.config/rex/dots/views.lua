-- How a view opens and closes. Every view goes through `open`, so its
-- placement is decided here and nowhere else.
local must = require("dots.result").must
local process = require("dots.process")
local sessions = require("dots.sessions")

local M = {}

-- Where a view floats: [x, y, w, h] as fractions of the window. A list
-- fills nearly the whole window; popups are harmless, so they err on the
-- large side.
M.LIST = { 0.04, 0.05, 0.92, 0.88 }

-- The directory holding this configuration, for scripts run with `rex do`.
local root = (debug.getinfo(1, "S").source:gsub("^@", "")):match("^(.*)/dots/views%.lua$")

---An open view, and the pane it returns to.
---@class dots.OpenView
---@field title string
---@field session_id string
---@field layer_id string
---@field origin string The block the user was in when the first view opened.
---@field zoomed boolean Whether `origin` was zoomed; focusing the view ended the zoom.
---@field client_id? string The app that opened the view; CLI connections are transient.

---Open views by their picker's block id. Module state, so a reload forgets
---them: a view opened before it still works, but Esc leaves focus to Rex.
---@type table<string, dots.OpenView>
M.open_views = {}

---@class dots.ViewSpec
---@field title string The view's block label, shown in its header.
---@field view? string The action that returns a picker view, such as `go_view`.
---@field command? string[] An interactive program to run instead of a picker. Like `dots-pick`, it takes `--dismiss ACTION` and runs that action when the user closes it.
---@field at rex.Context The action that opens it, run from the pane the user is in or from an open view.

---The program a view runs, closing through `views_dismiss`.
---@param spec dots.ViewSpec
---@return string[]
local function command(spec)
  local argv = {}
  if spec.command then
    if spec.view then
      error("opening " .. spec.title .. ": a view names a picker view or a command, not both", 0)
    end
    for _, arg in ipairs(spec.command) do
      argv[#argv + 1] = arg
    end
  else
    local view = spec.view
      or error("opening " .. spec.title .. ": a view names a picker view or a command", 0)
    argv[1] = process.which("dots-pick")
      or error("dots-pick is not installed on the command path", 0)
    argv[2] = view
  end
  argv[#argv + 1] = "--dismiss"
  argv[#argv + 1] = "views_dismiss"
  return argv
end

---The window holding `block_id` in `view`, and the floating layer holding it,
---if it floats.
---@param view rex.View
---@param block_id string
---@return rex.Window? window
---@return rex.Layer? floating
local function locate(view, block_id)
  for _, window in ipairs(view.windows or {}) do
    for _, layer in ipairs(window.layers or {}) do
      for _, placed in ipairs(layer.blocks or {}) do
        if placed.block_id == block_id then
          return window, layer.kind == "floating" and layer or nil
        end
      end
    end
  end
  return nil, nil
end

---Back to the pane a view was opened from, zoomed again if it was. Done while
---the view is still open: once it exits, Rex would focus some other pane.
---@param entry dots.OpenView
local function restore(entry)
  local args = { session_id = entry.session_id, block_id = entry.origin }
  if entry.zoomed then
    args.zoom = true
    must("zooming the pane again", rex.session.zoom_block(args))
  else
    must("focusing the pane", rex.session.focus_block(args))
  end
end

---Open a view as a floating layer over the window holding the block the
---opening action ran from. Rex's own default is the session's active window,
---which can lag what the app shows after a jump, leaving the view behind an
---unseen tab. A window shows one view at a time: opening one closes any other
---there, and the key of the view already open closes it. A view replacing
---another returns to the pane the first was opened from, and runs in that
---pane's directory. Rex removes the layer when the program exits; closing
---it first runs `views_dismiss`.
---@param spec dots.ViewSpec
---@return string? layer_id The new layer, or nil when the key closed its view.
function M.open(spec)
  local argv = command(spec)
  local session_id = spec.at.session_id
    or error("opening " .. spec.title .. ": run it from a Rex session", 0)
  local here = spec.at.block_id or error("opening " .. spec.title .. ": run it from a pane", 0)
  local view = must("opening " .. spec.title, sessions.view(session_id))
  local window = locate(view, here) or error("opening " .. spec.title .. ": its pane is gone", 0)

  local from = M.open_views[here]
  local client_id = from and from.client_id
  if not client_id and spec.at.client_id then
    local clients = must("identifying the opening app", rex.call("client.list"))
    for _, client in ipairs(clients.clients or {}) do
      if client.client_id == spec.at.client_id and client.info and client.info.kind == "app" then
        client_id = client.client_id
        break
      end
    end
  end
  local origin = from and from.origin or here
  local zoomed = from and from.zoomed or window.zoomed_block_id == here
  local toggled = from ~= nil and from.title == spec.title
  if toggled then
    restore(from)
  end
  for block_id, entry in pairs(M.open_views) do
    if entry.session_id == session_id then
      local placed, floating = locate(view, block_id)
      if not placed then
        M.open_views[block_id] = nil
      elseif placed == window and floating then
        M.open_views[block_id] = nil
        must(
          "closing " .. entry.title,
          rex.session.close_layer({ session_id = session_id, layer_id = floating.layer_id })
        )
      end
    end
  end
  if toggled then
    return nil
  end

  local cwd = sessions.cwd(session_id, origin)
  local created = must(
    "opening " .. spec.title,
    rex.session.new_layer({
      session_id = session_id,
      window_id = window.window_id,
      bounds = M.LIST,
      focus = true,
      layout = rex.layout.block({
        flavor = "com.superlogical.terminal.shell",
        label = spec.title,
        options = { command = argv, cwd = cwd },
      }),
    })
  )
  M.open_views[created.block_ids[1]] = {
    title = spec.title,
    session_id = session_id,
    layer_id = created.layer_id,
    origin = origin,
    zoomed = zoomed,
    client_id = client_id,
  }
  return created.layer_id
end

---Keep the opening app when a picker invokes its next action through a CLI.
---@param ctx rex.Context
---@return rex.Context
function M.context(ctx)
  local entry = ctx.block_id and M.open_views[ctx.block_id]
  if not entry or not entry.client_id then
    return ctx
  end
  local origin = {}
  for key, value in pairs(ctx) do
    origin[key] = value
  end
  origin.client_id = entry.client_id
  return origin
end

---Show a clickable URL on the viewing client, without opening a server browser.
---@param ctx rex.Context
---@param url string
function M.link(ctx, url)
  if not url:match("^https://[!-~]+$") then
    error("link must be an HTTPS URL without spaces or control characters", 0)
  end
  local printf = process.which("printf") or error("printf is not on the command path", 0)
  local text = "\27]8;;" .. url .. "\7" .. url .. "\27]8;;\7\n"
  must(
    "showing the link",
    rex.session.new_window({
      session_id = ctx.session_id,
      window_label = "link",
      layout = rex.layout.block({
        flavor = "com.superlogical.terminal.shell",
        options = {
          command = { printf, "%s", text },
          shell = "none",
          login = false,
          exit = { on_completion = false },
        },
      }),
    })
  )
end

---Closing a view: back to the pane it was opened from, while the view still
---shows. A view this load did not open, or whose pane has closed, leaves
---focus to Rex.
---@param ctx rex.Context The picker's.
function M.dismiss(ctx)
  local entry = ctx.block_id and M.open_views[ctx.block_id]
  if not entry then
    return
  end
  M.open_views[ctx.block_id] = nil
  local view = must("dismissing " .. entry.title, sessions.view(entry.session_id))
  if locate(view, entry.origin) then
    restore(entry)
  end
end

function M.install()
  rex.action({
    name = "views_dismiss",
    title = "Views: dismiss",
    description = "Return from a closing view to the pane it was opened from, for the view's program",
    category = "Navigation",
    run = M.dismiss,
  })
end

---A preview command showing a terminal's screen, colors included: `target`
---selects it, such as `{ "-s", session, "-w", window }`.
---@param target string[]
---@return string[]
function M.capture(target)
  local argv = { "rex", "capture" }
  for _, arg in ipairs(target) do
    argv[#argv + 1] = arg
  end
  argv[#argv + 1] = "--format=vt"
  argv[#argv + 1] = "--trim"
  return argv
end

---The path of a script under `scripts/`.
---@param name string
---@return string
function M.script(name)
  return (root or ".") .. "/scripts/" .. name .. ".lua"
end

---Keeps a view current while it is open: the picker reads the view again
---after any of `events`, and re-runs the selected preview every second. The
---watch script lives exactly as long as its picker, which stops it.
---@param view pick.View
---@param events string[] Rex events that may change the view's rows.
---@return pick.View view
function M.live(view, events)
  local rex_cli = process.which("rex") or "rex"
  local args = must("encoding watch events", rex.json.encode({ events = events }))
  view.watch = { rex_cli, "do", M.script("watch"), "--all", "--args", args }
  view.preview_every = 1
  return view
end

return M
