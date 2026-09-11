local binding = require("herdr_review.binding")
local client = require("herdr_review.client")
local codediff = require("herdr_review.codediff")
local composer = require("herdr_review.composer")
local location = require("herdr_review.location")
local picker = require("herdr_review.picker")
local render = require("herdr_review.render")

local M = {}
local states = {}
local watchers = {}
local refreshing = {}
local discovering = {}
local pending_navigation
local options = {}
local watch_binding
local loading = {}

local function read_binding(checkout_root, binding_id, callback)
  if loading[binding_id] then
    table.insert(loading[binding_id], callback)
    return
  end
  loading[binding_id] = { callback }
  client.load_binding(checkout_root, binding_id, function(state, err)
    local waiting = loading[binding_id]
    loading[binding_id] = nil
    for _, done in ipairs(waiting) do done(state, err) end
  end, states[binding_id] and states[binding_id].state)
end

local function notify(message, level)
  vim.notify("review: " .. message, level or vim.log.levels.INFO)
end

local function error_message(err)
  return type(err) == "table" and (err.message or err.code or "unknown review error") or err or "unknown review error"
end

local function invoke(operation, callback)
  operation(function(result, err)
    if err then
      notify(error_message(err), vim.log.levels.ERROR)
    end
    if callback then
      callback(result, err)
    end
  end)
end

local function author()
  return options.author or vim.env.USER or "reviewer"
end

local function binding_id_from_state(state)
  return state and state.binding and state.binding.id
end

local function validate_state(expected_binding, state)
  local actual = binding_id_from_state(state)
  if not actual then
    return nil, { code = "invalid_response", message = "review bridge did not return a runtime binding" }
  end
  if expected_binding and actual ~= expected_binding then
    return nil, {
      code = "binding_identity_mismatch",
      message = string.format("review bridge returned binding %s while this tab owns %s", actual, expected_binding),
    }
  end
  return state, nil
end

local function still_bound(view, binding_id)
  return binding.get(view.tabpage) == binding_id and codediff.view(view.tabpage) ~= nil
end

local function accept_state(view, state, coherent)
  local current = codediff.view(view.tabpage)
  if not current or not codediff.same_view(view, current) then return state end
  local binding_id = binding_id_from_state(state)
  binding.set(view.tabpage, binding_id)
  local cached = states[binding_id] or { coherent_tabs = {} }
  cached.coherent_tabs = cached.coherent_tabs or {}
  local previous_observation = cached.state and cached.state.observation and cached.state.observation.id
  local next_observation = state.observation and state.observation.id
  local observation_changed = previous_observation and previous_observation ~= next_observation
  if observation_changed then
    cached.coherent_tabs[view.tabpage] = false
  end
  cached.checkout_root = view.checkout_root
  cached.state = state
  cached.loaded_at = vim.uv.now()
  states[binding_id] = cached
  local view_is_coherent = coherent
    or codediff.matches_observation(view.comparison, state.observation)
    or cached.coherent_tabs[view.tabpage]
  cached.coherent_tabs[view.tabpage] = view_is_coherent and true or false
  if view_is_coherent then
    render.refresh(view.tabpage, view, state, true)
  elseif observation_changed then
    render.refresh(view.tabpage, view, nil)
  end
  watch_binding(view.checkout_root, binding_id)
  return state
end

local function load_state(view, callback)
  local binding_id = binding.get(view.tabpage)
  if not binding_id then
    callback(nil, nil)
    return
  end
  read_binding(view.checkout_root, binding_id, function(state, err)
    if not still_bound(view, binding_id) then callback(nil, nil); return end
    if not codediff.same_view(view, codediff.view(view.tabpage)) then
      callback(nil, { code = "view_changed", message = "review comparison changed; try the action again" })
      return
    end
    if err then
      callback(nil, err)
      return
    end
    state, err = validate_state(binding_id, state)
    if state then
      accept_state(view, state, false)
    end
    callback(state, err)
  end)
end

local function observe_state(view, callback)
  local binding_id = binding.get(view.tabpage)
  if not binding_id then
    callback(nil, nil)
    return
  end
  client.observe_binding(view.checkout_root, binding_id, view.comparison, function(state, err)
    if not still_bound(view, binding_id) then callback(nil, nil); return end
    if err then
      callback(nil, err)
      return
    end
    state, err = validate_state(binding_id, state)
    if state then
      local latest = codediff.view(view.tabpage)
      if latest and not codediff.same_view(view, latest) then
        if codediff.matches_observation(latest.comparison, state.observation) then
          accept_state(latest, state, false)
          callback(state, nil)
        else
          observe_state(latest, callback)
        end
        return
      end
      accept_state(view, state, latest ~= nil)
    end
    callback(state, err)
  end)
end

local function load_coherent_state(view, callback)
  load_state(view, function(state, err)
    if err or not state or codediff.matches_observation(view.comparison, state.observation) then
      callback(state, err)
      return
    end
    observe_state(view, callback)
  end)
end

local function latest_attempt(request)
  local attempts = request and request.attempts or {}
  return attempts[#attempts]
end

watch_binding = function(checkout_root, binding_id)
  if watchers[binding_id] then return end
  local watcher = { requests = {}, failures = 0 }
  for _, request in ipairs((states[binding_id] and states[binding_id].state.agent_requests) or {}) do
    watcher.requests[request.id] = request.state .. ":" .. #(request.answered_thread_ids or {})
  end
  watchers[binding_id] = watcher
  local function poll()
    if watchers[binding_id] ~= watcher then return end
    local views = {}
    for _, tabpage in ipairs(vim.api.nvim_list_tabpages()) do
      if binding.get(tabpage) == binding_id then
        local view = codediff.view(tabpage)
        if view then table.insert(views, view) end
      end
    end
    if #views == 0 then
      watchers[binding_id] = nil
      return
    end
    read_binding(checkout_root, binding_id, function(state, err)
      if state then state, err = validate_state(binding_id, state) end
      if err or not state then
        watcher.failures = watcher.failures + 1
        if err and watcher.failures == 1 then notify(error_message(err), vim.log.levels.WARN) end
        vim.defer_fn(poll, math.min(30000, (options.poll_interval_ms or 2000) * 2 ^ watcher.failures))
        return
      end
      watcher.failures = 0
      for _, request in ipairs(state.agent_requests or {}) do
        local replies = #(request.answered_thread_ids or {})
        local signature = request.state .. ":" .. replies
        local changed = watcher.requests[request.id] ~= signature
        watcher.requests[request.id] = signature
        if changed and binding.owns_request(request, binding_id) and request.state == "returned" then
          local expected = #(request.thread_ids or {})
          notify(string.format("request %d: %d/%d thread replies saved%s", request.ordinal, replies, expected,
              replies < expected and "; use :ReviewRetryDispatch to recover missing answers" or ""),
            replies < expected and vim.log.levels.WARN or vim.log.levels.INFO)
        elseif changed and binding.owns_request(request, binding_id) and request.state == "awaiting_retry" then
          local attempt = latest_attempt(request) or {}
          local detail = attempt.detail and (": " .. attempt.detail) or ""
          notify("agent dispatch " .. (attempt.state or "failed") .. detail, vim.log.levels.WARN)
        end
      end
      local previous = states[binding_id]
      local unchanged = previous and vim.deep_equal(previous.state, state)
      for _, view in ipairs(views) do
        local current = codediff.view(view.tabpage)
        if binding.get(view.tabpage) == binding_id and current and codediff.same_view(view, current) then
          local cached = states[binding_id]
          local coherent = cached and cached.coherent_tabs[view.tabpage]
            and cached.state.observation.id == state.observation.id
          if coherent or codediff.matches_observation(view.comparison, state.observation) then
            if not unchanged then accept_state(view, state, false) end
          else
            observe_state(view, function(_, observe_err)
              if observe_err then notify(error_message(observe_err), vim.log.levels.ERROR) end
            end)
          end
        end
      end
      if states[binding_id] then states[binding_id].loaded_at = vim.uv.now() end
      vim.defer_fn(poll, options.poll_interval_ms or 2000)
    end)
  end
  vim.defer_fn(poll, options.poll_interval_ms or 2000)
end

local function current_view()
  local view, err = codediff.view_for_buffer()
  if view then
    return view
  end
  return codediff.view(vim.api.nvim_get_current_tabpage())
end

local function choose_agent(view, binding_id, prompt, callback)
  invoke(function(done)
    client.list_agents(view.checkout_root, binding_id, done)
  end, function(agents, err)
    if err then
      return
    end
    local available = vim.tbl_filter(function(agent)
      return agent.status == "idle" or agent.status == "done"
    end, agents or {})
    if #available == 0 then
      notify("this Herdr workspace has no idle agents", vim.log.levels.WARN)
      return
    end
    vim.ui.select(available, {
      prompt = prompt,
      format_item = function(agent)
        local assignment = agent.assignment or {}
        local kind = assignment.expected_agent_kind and (" · " .. assignment.expected_agent_kind) or ""
        return string.format("%s%s · %s", agent.label or assignment.pane_id or "agent", kind, agent.status or "unknown")
      end,
    }, function(agent)
      if agent then
        callback(agent.assignment, agent)
      end
    end)
  end)
end

function M.setup(opts)
  options = opts or {}
end

function M.refresh(callback)
  local view, err = current_view()
  if not view then
    notify(err, vim.log.levels.WARN)
    return
  end
  local binding_id = binding.get(view.tabpage)
  if not binding_id then
    render.refresh(view.tabpage, view, nil)
    if callback then
      callback(nil, nil)
    end
    return
  end
  if refreshing[binding_id] then
    table.insert(refreshing[binding_id], callback or false)
    return
  end
  refreshing[binding_id] = { callback or false }
  invoke(function(done)
    observe_state(view, done)
  end, function(state, refresh_err)
    local waiting = refreshing[binding_id] or {}
    refreshing[binding_id] = nil
    for _, next_callback in ipairs(waiting) do
      if next_callback then
        next_callback(state, refresh_err)
      end
    end
  end)
end

function M.create_thread(line1, line2)
  local view, err = codediff.capture(line1, line2)
  if not view then
    notify(err, vim.log.levels.ERROR)
    return
  end
  composer.open(view, function(body)
    local binding_id = binding.get(view.tabpage)
    invoke(function(done)
      client.create_thread(view.checkout_root, binding_id, view.comparison, {
        anchor = {
          original = {
            path = view.path,
            side = view.side,
            start_line = view.start_line,
            end_line = view.end_line,
          },
          context = view.anchor_context,
        },
        body = body,
        author = author(),
      }, done)
    end, function(state, create_err)
      if create_err then
        return
      end
      state, create_err = validate_state(binding_id, state)
      if not state then
        notify(error_message(create_err), vim.log.levels.ERROR)
        return
      end
      local latest = codediff.view(view.tabpage)
      if latest and codediff.same_view(view, latest) then
        accept_state(view, state, true)
      elseif latest then
        accept_state(view, state, false)
        observe_state(latest, function(_, observe_err)
          if observe_err then notify(error_message(observe_err), vim.log.levels.ERROR) end
        end)
      else
        accept_state(view, state, false)
      end
      local threads = state.threads or {}
      local thread = threads[#threads]
      notify("thread " .. (thread and thread.id:sub(1, 8) or "created") .. " saved")
    end)
  end)
end

local function choose_thread(candidates, prompt, callback)
  if #candidates == 0 then
    notify("no review thread is selected", vim.log.levels.WARN)
  elseif #candidates == 1 then
    callback(candidates[1])
  else
    vim.ui.select(candidates, {
      prompt = prompt,
      format_item = function(thread)
        local messages = thread.messages or {}
        return (messages[#messages] and messages[#messages].body or "thread"):gsub("\n", " ")
      end,
    }, callback)
  end
end

local function resolve_thread(view, thread)
  local binding_id = binding.get(view.tabpage)
  if not binding_id then
    notify("this diff has no review activity", vim.log.levels.WARN)
    return
  end
  invoke(function(done)
    client.resolve_thread(view.checkout_root, binding_id, thread.id, done)
  end, function(_, resolve_err)
    if not resolve_err then
      M.refresh()
    end
  end)
end

function M.resolve_at_cursor()
  local view, err = current_view()
  if not view then
    notify(err, vim.log.levels.ERROR)
    return
  end
  local candidates = vim.tbl_filter(function(thread)
    return thread.status == "open"
  end, render.threads_at_cursor())
  choose_thread(candidates, "Resolve review thread", function(thread)
    if thread then
      resolve_thread(view, thread)
    end
  end)
end

function M.reply_at_cursor()
  local view, err = current_view()
  if not view then
    notify(err, vim.log.levels.ERROR)
    return
  end
  local binding_id = binding.get(view.tabpage)
  if not binding_id then
    notify("this diff has no review activity", vim.log.levels.WARN)
    return
  end
  local candidates = vim.tbl_filter(function(thread)
    return thread.status == "open"
  end, render.threads_at_cursor())
  choose_thread(candidates, "Reply to review thread", function(thread)
    if not thread then
      return
    end
    local target = render.location(thread, view.tabpage) or location.target(thread) or {}
    composer.open({ path = target.path or "review", start_line = location.line(target) or 1 }, function(body)
      invoke(function(done)
        client.add_message(view.checkout_root, binding_id, thread.id, {
          body = body,
          author = author(),
        }, done)
      end, function(_, reply_err)
        if not reply_err then
          M.refresh()
        end
      end)
    end)
  end)
end

function M.request_agent()
  local view, err = current_view()
  if not view then
    notify(err, vim.log.levels.ERROR)
    return
  end
  local binding_id = binding.get(view.tabpage)
  if not binding_id then
    notify("add a review message before sending to an agent", vim.log.levels.WARN)
    return
  end
  local thread_ids, selected_agent
  local finished = false
  local function dispatch_when_ready()
    if finished or not thread_ids or not selected_agent then
      return
    end
    finished = true
    local latest = codediff.view(view.tabpage)
    if binding.get(view.tabpage) ~= binding_id or not latest or not codediff.same_view(view, latest) then
      notify("review view changed; send review messages again", vim.log.levels.WARN)
      return
    end
    local assignment = selected_agent.assignment
    invoke(function(done)
      client.create_agent_request(view.checkout_root, binding_id, thread_ids, assignment, done)
    end, function(request, request_err)
      if not request_err then
        notify("agent request " .. request.ordinal .. " queued for " .. (selected_agent.label or assignment.pane_id))
        watch_binding(view.checkout_root, binding_id)
        M.refresh()
      end
    end)
  end
  -- Queue discovery first so its independent bridge worker can run while the
  -- ordered store lane loads/observes the review. Never dispatch from cached state.
  choose_agent(view, binding_id, "Send review messages to agent", function(_, agent)
    selected_agent = agent
    dispatch_when_ready()
  end)
  load_coherent_state(view, function(state, load_err)
    if load_err then
      finished = true
      notify(error_message(load_err), vim.log.levels.ERROR)
      return
    end
    if not state then
      finished = true
      notify("add a review message before sending to an agent", vim.log.levels.WARN)
      return
    end
    thread_ids = {}
    for _, thread in ipairs(state.threads or {}) do
      if thread.status == "open" then
        table.insert(thread_ids, thread.id)
      end
    end
    if #thread_ids == 0 then
      finished = true
      notify("there are no open review threads to send", vim.log.levels.WARN)
      return
    end
    dispatch_when_ready()
  end)
end

function M.retry_dispatch()
  local view, err = current_view()
  if not view then
    notify(err, vim.log.levels.ERROR)
    return
  end
  load_state(view, function(state, load_err)
    if load_err or not state then
      if load_err then
        notify(error_message(load_err), vim.log.levels.ERROR)
      end
      return
    end
    local outstanding = vim.tbl_filter(function(request)
      return request.recovery == "retry" or request.recovery == "inspect_before_retry"
    end, state.agent_requests or {})
    if #outstanding == 0 then
      notify("there are no outstanding agent requests to recover", vim.log.levels.WARN)
      return
    end
    local function recover(pending)
      if not pending then return end
      local function retry_with_agent()
        choose_agent(view, state.binding.id, "Retry agent dispatch", function(assignment, agent)
          invoke(function(done)
            client.retry_dispatch(view.checkout_root, state.binding.id, pending.id, assignment, done)
          end, function(_, retry_err)
            if not retry_err then
              notify("dispatch retry queued for " .. (agent.label or assignment.pane_id))
              watch_binding(view.checkout_root, state.binding.id)
              M.refresh()
            end
          end)
        end)
      end
      if pending.recovery == "inspect_before_retry" then
        vim.ui.select({ "Cancel and inspect the agent", "Retry anyway" }, {
          prompt = "The previous request may have run. Retry only after inspecting the agent/worktree.",
        }, function(choice)
          if choice == "Retry anyway" then
            retry_with_agent()
          end
        end)
      else
        retry_with_agent()
      end
    end
    vim.ui.select(outstanding, {
      prompt = "Recover an outstanding review request",
      format_item = function(request)
        return string.format("Request %d · %s · %d/%d answers", request.ordinal, request.state,
          #(request.answered_thread_ids or {}), #(request.thread_ids or {}))
      end,
    }, recover)
  end)
end

function M.status()
  local view, err = current_view()
  if not view then
    notify(err, vim.log.levels.ERROR)
    return
  end
  load_state(view, function(state, load_err)
    if load_err then
      notify(error_message(load_err), vim.log.levels.ERROR)
    elseif not state then
      notify("this diff has no review activity")
    else
      local open, resolved = 0, 0
      for _, thread in ipairs(state.threads or {}) do
        if thread.status == "open" then open = open + 1 else resolved = resolved + 1 end
      end
      local request = render.request_summary(state, state.binding.id)
      notify(string.format("%d open · %d resolved%s", open, resolved, request and (" · " .. request) or ""))
    end
  end)
end

local function live_location(thread, tabpage)
  return render.location(thread, tabpage) or location.target(thread)
end

local function jump_to_thread(view, thread)
  if not location.target(thread) then
    picker.show(thread)
    return
  end
  local resolution = type(thread.resolution) == "table" and thread.resolution or {}
  if (resolution.status == "deleted" or resolution.status == "ambiguous" or resolution.status == "unavailable")
    and type(resolution.current_location) ~= "table" then
    picker.show(thread)
    notify("anchor is " .. resolution.status .. "; showing saved evidence instead", vim.log.levels.WARN)
    return
  end
  local target = live_location(thread, view.tabpage)
  if not target then
    local status = thread.resolution and thread.resolution.status or "unavailable"
    notify("anchor is " .. status .. "; inspect its original context in the thread picker", vim.log.levels.WARN)
    return
  end
  codediff.jump(view.tabpage, target, location.line(target), function(jump_err)
    if jump_err then
      notify(jump_err, vim.log.levels.WARN)
    end
  end)
end

local function open_thread(context, thread)
  if not location.target(thread) then
    picker.show(thread)
    return
  end
  local navigation = { context = context, thread = thread }
  pending_navigation = navigation
  local ok, err = codediff.open(context.state.observation.comparison)
  if not ok then
    pending_navigation = nil
    notify(err, vim.log.levels.ERROR)
    return
  end
  -- CodeDiff readiness drives the jump through attach; this is only a deadline,
  -- never a guess about how long Git or diff computation should take.
  vim.defer_fn(function()
    if pending_navigation == navigation then
      pending_navigation = nil
      notify("the review diff is not ready; select the thread again when it opens", vim.log.levels.WARN)
    end
  end, 15000)
end

function M.pick_threads()
  local view = current_view()
  if not view then
    local root = vim.fs.root(0, ".git") or vim.fn.getcwd()
    client.load_context(root, function(context, err)
      if err then
        notify(error_message(err), vim.log.levels.ERROR)
      elseif not context then
        notify("this checkout has no bound review context", vim.log.levels.WARN)
      else
        picker.open(context.checkout_root, context.state, {
          jump = function(thread)
            if (vim.fs.root(0, ".git") or vim.fn.getcwd()) ~= root then
              notify("checkout changed; select the review again", vim.log.levels.WARN)
              return
            end
            open_thread(context, thread)
          end,
          resolve = function(thread)
            invoke(function(done)
              client.resolve_thread(context.checkout_root, context.state.binding.id, thread.id, done)
            end)
          end,
        })
      end
    end)
    return
  end
  load_state(view, function(state, load_err)
    if load_err then
      notify(error_message(load_err), vim.log.levels.ERROR)
    elseif not state then
      notify("this diff has no review activity", vim.log.levels.WARN)
    else
      picker.open(view.checkout_root, state, {
        jump = function(thread) jump_to_thread(view, thread) end,
        resolve = function(thread) resolve_thread(view, thread) end,
      })
    end
  end)
end

function M.open_review(opts)
  local tabpage = vim.api.nvim_get_current_tabpage()
  local root = vim.fs.root(0, ".git") or vim.fn.getcwd()
  local function open(comparison, err)
    if vim.api.nvim_get_current_tabpage() ~= tabpage
      or (vim.fs.root(0, ".git") or vim.fn.getcwd()) ~= root then
      notify("checkout changed; open the diff again", vim.log.levels.WARN)
    elseif err then
      notify(error_message(err), vim.log.levels.ERROR)
    else
      local ok, open_err = codediff.open(comparison)
      if not ok then notify(open_err, vim.log.levels.ERROR) end
    end
  end
  client.load_context(root, function(context, err)
    if err then
      notify(error_message(err), vim.log.levels.ERROR)
    elseif not context and opts and opts.branch_fallback then
      require("herdr_review.branch").comparison(root, open)
    elseif not context then
      notify("this checkout has no bound review context", vim.log.levels.WARN)
    else
      open(context.state.observation.comparison)
    end
  end)
end

function M.navigate(direction)
  local view, err = current_view()
  if not view then
    notify(err, vim.log.levels.WARN)
    return
  end
  load_state(view, function(state, load_err)
    if load_err or not state then
      if load_err then notify(error_message(load_err), vim.log.levels.ERROR) end
      return
    end
    local threads = vim.tbl_filter(function(thread)
      return thread.status == "open" and live_location(thread, view.tabpage) ~= nil
    end, state.threads or {})
    table.sort(threads, function(left, right)
      local left_copy, right_copy = vim.deepcopy(left), vim.deepcopy(right)
      left_copy.resolution = { current_location = live_location(left, view.tabpage) }
      right_copy.resolution = { current_location = live_location(right, view.tabpage) }
      return location.compare(left_copy, right_copy)
    end)
    if #threads == 0 then
      notify("there are no open review threads")
      return
    end
    local here = codediff.view_for_buffer()
    local cursor = vim.api.nvim_win_get_cursor(0)[1]
    local selected
    local function after(target)
      return not here or target.path > here.path
        or (target.path == here.path and target.side > here.side)
        or (target.path == here.path and target.side == here.side and (location.line(target) or 0) > cursor)
    end
    if direction > 0 then
      selected = vim.iter(threads):find(function(thread) return after(live_location(thread, view.tabpage)) end) or threads[1]
    else
      for index = #threads, 1, -1 do
        local target = live_location(threads[index], view.tabpage)
        if not after(target) and (not here or location.line(target) < cursor or target.path ~= here.path or target.side ~= here.side) then
          selected = threads[index]
          break
        end
      end
      selected = selected or threads[#threads]
    end
    jump_to_thread(view, selected)
  end)
end

function M.attach(tabpage, force_refresh)
  tabpage = tabpage or vim.api.nvim_get_current_tabpage()
  local view = codediff.view(tabpage)
  if not view then
    M.detach(tabpage, false)
    return
  end
  local navigation = pending_navigation
  if navigation and view.checkout_root == navigation.context.checkout_root
    and codediff.matches_observation(view.comparison, navigation.context.state.observation) then
    pending_navigation = nil
    accept_state(view, navigation.context.state, true)
    vim.schedule(function()
      local latest = codediff.view(tabpage)
      if latest and codediff.same_view(view, latest) then
        jump_to_thread(latest, navigation.thread)
      end
    end)
  end
  codediff.bind_keymaps(tabpage, {
    create_thread = function() M.create_thread() end,
    create_range_thread = function() M.create_thread(vim.fn.line("v"), vim.fn.line(".")) end,
    resolve_thread = M.resolve_at_cursor,
    reply_thread = M.reply_at_cursor,
    refresh = M.refresh,
    status = M.status,
    request_agent = M.request_agent,
    previous_thread = function() M.navigate(-1) end,
    next_thread = function() M.navigate(1) end,
  })
  local binding_id = binding.get(tabpage)
  if not binding_id then
    render.refresh(tabpage, view, nil)
    if discovering[tabpage] then return end
    discovering[tabpage] = true
    client.load_context(view.checkout_root, function(context, err)
      discovering[tabpage] = nil
      if err then
        notify(error_message(err), vim.log.levels.ERROR)
        return
      end
      local latest = codediff.view(tabpage)
      if context and latest and not binding.get(tabpage)
        and codediff.same_view(view, latest)
        and codediff.matches_observation(latest.comparison, context.state.observation) then
        accept_state(latest, context.state, true)
      end
    end)
    return
  end
  local cached = states[binding_id]
  local cached_is_coherent = cached and cached.coherent_tabs and cached.coherent_tabs[tabpage]
  local stale = not cached or not cached.loaded_at
    or vim.uv.now() - cached.loaded_at >= (options.poll_interval_ms or 2000)
  if cached and cached.state
    and (cached_is_coherent or codediff.matches_observation(view.comparison, cached.state.observation)) then
    render.refresh(tabpage, view, cached.state, true)
    watch_binding(view.checkout_root, binding_id)
  end
  if force_refresh or stale or not cached_is_coherent then
    load_coherent_state(view, function(_, load_err)
      if load_err then
        notify(error_message(load_err), vim.log.levels.ERROR)
      end
    end)
  end
end

function M.attach_for_buffer(bufnr)
  local tabpage = codediff.tab_for_buffer(bufnr)
  if tabpage then
    M.attach(tabpage)
  end
end

function M.detach(tabpage, forget)
  tabpage = tabpage or vim.api.nvim_get_current_tabpage()
  local binding_id = binding.get(tabpage)
  local cached = binding_id and states[binding_id]
  if cached and cached.coherent_tabs then
    cached.coherent_tabs[tabpage] = false
  end
  codediff.release_keymaps(tabpage)
  render.detach(tabpage)
  if forget then
    binding.clear(tabpage)
  end
end

function M.rerender()
  render.reflow()
end

return M
