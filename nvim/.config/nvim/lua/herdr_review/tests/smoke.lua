local config = assert(vim.env.HERDR_REVIEW_NVIM_CONFIG, "HERDR_REVIEW_NVIM_CONFIG must name the Neovim config directory")
package.path = config .. "/lua/?.lua;" .. config .. "/lua/?/init.lua;" .. package.path

local wrap = require("herdr_review.wrap")
assert(vim.deep_equal(wrap.lines("one two three", 7), { "one two", "three" }))
assert(vim.deep_equal(wrap.lines("", 7), { "" }))
for _, text in ipairs({ "long_identifier_without_spaces", "你好世界你好", "café café", "👩‍💻 review text" }) do
  local lines = wrap.lines(text, 12)
  for _, line in ipairs(lines) do
    assert(vim.fn.strdisplaywidth(line) <= 12, "review prose must fit its available cells")
  end
  assert(table.concat(lines):gsub("%s", "") == text:gsub("%s", ""), "wrapping must preserve text")
end

local checkout = vim.env.HERDR_REVIEW_CHECKOUT or "/tmp/review-smoke"
local external_bridge = vim.env.HERDR_REVIEW_BINARY
local fake_bridge
if external_bridge then
  vim.g.herdr_review_binary = external_bridge
else
  fake_bridge = vim.fn.tempname()
  vim.fn.writefile({
    "#!/bin/sh",
    "count=0",
    "while IFS= read -r frame; do",
    "  count=$((count + 1))",
    "  if [ \"$count\" -eq 1 ]; then",
    "    case \"$frame\" in *'\"method\":\"hello\"'*) ;; *) exit 9 ;; esac",
    "    printf '%s\\n' '{\"id\":1,\"result\":{\"protocol\":1,\"capabilities\":[\"binding.load\"]}}'",
    "  else",
    "    case \"$frame\" in *'\"method\":\"binding.load\"'*) ;; *) exit 10 ;; esac",
    "    case \"$frame\" in *'\"binding_id\":\"00000000000000000000000000000000\"'*) printf '%s\\n' '{\"id\":2,\"result\":null}' ;; *) exit 10 ;; esac",
    "  fi",
    "done",
  }, fake_bridge)
  vim.fn.setfperm(fake_bridge, "rwx------")
  vim.g.herdr_review_binary = fake_bridge
end

local client = require("herdr_review.client")
local bridge_done, bridge_error = false, nil
client.load_binding(checkout, "00000000000000000000000000000000", function(result, err)
  if external_bridge then
    assert(result == nil and err and err.code == "not_found", "an unknown binding must be rejected")
  else
    assert(result == nil and err == nil)
  end
  bridge_error, bridge_done = err, true
end)
assert(vim.wait(external_bridge and 5000 or 1000, function() return bridge_done end), "typed client request timed out")
if not external_bridge then
  assert(bridge_error == nil)
end
client.stop(checkout)
if fake_bridge then vim.fn.delete(fake_bridge) end
vim.g.herdr_review_binary = nil

local tabpage = vim.api.nvim_get_current_tabpage()
local base_buf = vim.api.nvim_create_buf(false, true)
local target_buf = vim.api.nvim_create_buf(false, true)
vim.api.nvim_buf_set_lines(base_buf, 0, -1, false, { "base", "same" })
vim.api.nvim_buf_set_lines(target_buf, 0, -1, false, { "target", "same" })

local session = {
  git_root = checkout,
  original = { relative = "src/lib.rs" },
  modified = { relative = "src/lib.rs" },
  original_bufnr = base_buf,
  modified_bufnr = target_buf,
  original_revision = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  modified_revision = "WORKING",
  stored_diff_result = {},
}
local panel_view
local scoped_keymaps = {}
local released_keymaps = 0
local create_session = function() return "created" end
local update_diff_result = function(_, result)
  session.stored_diff_result = result
  return true
end

package.preload["codediff.ui.lifecycle"] = function()
  return {
    get_session = function(candidate) return candidate == tabpage and session or nil end,
    get_buffers = function() return session.original_bufnr, session.modified_bufnr end,
    get_paths = function() return session.original, session.modified end,
    get_windows = function() return nil, vim.api.nvim_get_current_win() end,
    find_tabpage_by_buffer = function(bufnr)
      return (bufnr == base_buf or bufnr == target_buf) and tabpage or nil
    end,
    get_panel_name = function() return panel_view and "explorer" or nil end,
    get_panel_view = function() return panel_view end,
    begin_keymap_scope = function() end,
    set_buf_keymap = function(_, _, _, lhs, rhs) scoped_keymaps[lhs] = rhs end,
    end_keymap_scope = function() end,
    release_keymap_scope = function() released_keymaps = released_keymaps + 1 end,
    create_session = function(...) return create_session(...) end,
    update_diff_result = function(...) return update_diff_result(...) end,
  }
end

local binding = require("herdr_review.binding")
local codediff = require("herdr_review.codediff")
local location = require("herdr_review.location")
local render = require("herdr_review.render")

local view = assert(codediff.view(tabpage))
assert(vim.deep_equal(view.comparison, {
  base = { kind = "commit", oid = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" },
  target = { kind = "working_tree" },
}), "CodeDiff must expose one normalized comparison")

vim.api.nvim_set_current_buf(target_buf)
local captured = assert(codediff.capture(1, 2))
assert(captured.target.start_line == 1 and captured.target.end_line == 2)
assert(vim.deep_equal(captured.anchor_context.selected, { "target", "same" }))

binding.set(tabpage, "binding-one")
vim.cmd.tabnew()
local other_tab = vim.api.nvim_get_current_tabpage()
binding.set(other_tab, "binding-two")
assert(binding.get(other_tab) == "binding-two" and binding.get(tabpage) == "binding-one", "binding identity must be tab-local")
binding.clear(other_tab)
vim.cmd.tabclose()
assert(binding.get(tabpage) == "binding-one")
binding.clear(tabpage)

local thread = {
  id = "thread-one",
  status = "open",
  anchor = {
    original = { path = "src/lib.rs", side = "target", start_line = 1, end_line = 2 },
    context = { selected = { "target", "same" } },
  },
  resolution = {
    status = "exact",
    current_location = { path = "src/lib.rs", side = "target", start_line = 1, end_line = 2 },
  },
  messages = { { author = "reviewer", body = "change this" } },
}
local state = {
  binding = { id = "binding-one" },
  context = { id = "context-one", title = "A review" },
  observation = {
    id = "observation-one",
    comparison = {
      base = { source = view.comparison.base, oid = view.comparison.base.oid },
      target = { source = view.comparison.target, oid = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" },
    },
  },
  threads = { thread },
  agent_requests = {},
}
assert(not codediff.matches_observation(view.comparison, state.observation), "mutable sources must not equal a captured observation by kind")
render.refresh(tabpage, view, state)
assert(#vim.api.nvim_buf_get_extmarks(target_buf, -1, 0, -1, {}) == 0, "an unverified mutable snapshot must not render anchors")
render.refresh(tabpage, view, state, true)
assert(render.count_for_path("src/lib.rs", tabpage) == 1)
local marks = vim.api.nvim_buf_get_extmarks(target_buf, -1, 0, -1, { details = true })
local gutter_rows = {}
for _, mark in ipairs(marks) do
  if mark[4].sign_text and mark[4].sign_text:find("▌", 1, true) then
    table.insert(gutter_rows, mark[2])
  end
end
assert(vim.deep_equal(gutter_rows, { 0, 1 }), "a thread range must fill every covered gutter row")
assert(vim.iter(marks):any(function(mark) return mark[4].virt_lines_above == true end), "thread messages must render above their anchor")
do
  local other = vim.api.nvim_create_namespace("other-plugin-test")
  local untouched = { { { "Other plugin virtual line stays exactly as supplied", "Normal" } } }
  vim.api.nvim_buf_set_extmark(target_buf, other, 0, 0, { virt_lines = untouched })
  local original_body = thread.messages[1].body
  local original_wrap = vim.wo.wrap
  thread.messages[1].body = string.rep("Readable review prose with word boundaries. ", 10)
  render.refresh(tabpage, view, state, true)
  local review_marks = vim.api.nvim_buf_get_extmarks(target_buf, -1, 0, -1, { details = true })
  assert(vim.iter(review_marks):any(function(mark)
    return mark[4].virt_lines_above and #mark[4].virt_lines > 5
  end), "long review prose must become multiple review-owned virtual lines")
  assert(vim.deep_equal(vim.api.nvim_buf_get_extmarks(target_buf, other, 0, -1, { details = true })[1][4].virt_lines, untouched))
  assert(vim.wo.wrap == original_wrap, "inline wrapping must not change the code window's options")
  vim.api.nvim_buf_clear_namespace(target_buf, other, 0, -1)
  thread.messages[1].body = original_body
  render.refresh(tabpage, view, state, true)
end
vim.api.nvim_win_set_cursor(0, { 1, 0 })
assert(render.threads_at_cursor()[1].id == thread.id)
vim.api.nvim_buf_set_lines(target_buf, 0, 0, false, { "inserted" })
assert(render.location(thread, tabpage).start_line == 2, "the live thread location must follow buffer edits")
render.reflow()
assert(render.location(thread, tabpage).start_line == 2, "reflow must preserve edited anchor positions")
local incoming = vim.deepcopy(state)
table.insert(incoming.threads[1].messages, { id = "late-answer", author = "agent", body = "A late answer" })
render.refresh(tabpage, view, incoming, true)
assert(render.location(thread, tabpage).start_line == 2, "incoming replies must preserve live edited anchor positions")
vim.api.nvim_buf_set_lines(target_buf, 0, 1, false, {})

local workflow = require("herdr_review.workflow")
workflow.setup({ author = "wolf", poll_interval_ms = 10 })
local original_load = client.load_binding
local original_context = client.load_context
client.load_context = function(_, callback) callback(nil, nil) end
local original_create = client.create_thread
local original_add_message = client.add_message
local original_observe = client.observe_binding
local composer = require("herdr_review.composer")
local original_compose = composer.open
local loads, creates, observes = 0, 0, 0
client.load_binding = function() loads = loads + 1 end
workflow.attach(tabpage)
assert(loads == 0, "ordinary CodeDiff must remain ephemeral until a persistent action")
assert(scoped_keymaps["<leader>r"] and scoped_keymaps["<leader>ra"] and scoped_keymaps["<leader>rp"] and scoped_keymaps["<leader>rS"] and scoped_keymaps["[r"] and scoped_keymaps["]r"], "ordinary CodeDiff must receive review activity keymaps and the which-key prefix")

composer.open = function(_, callback) callback("first message") end
client.create_thread = function(root, binding_id, comparison, request, callback)
  creates = creates + 1
  assert(root == checkout and binding_id == nil, "the first thread must not infer a checkout-global binding")
  assert(vim.deep_equal(comparison, view.comparison))
  assert(request.anchor.original.start_line == 1 and request.anchor.original.end_line == 1)
  assert(request.body == "first message" and request.author == "wolf")
  callback(state, nil)
end
vim.api.nvim_set_current_buf(target_buf)
vim.api.nvim_win_set_cursor(0, { 1, 0 })
workflow.create_thread()
assert(creates == 1 and binding.get(tabpage) == "binding-one", "first thread must atomically return and bind the new activity")

client.observe_binding = function(root, binding_id, comparison, callback)
  observes = observes + 1
  assert(root == checkout and binding_id == "binding-one")
  assert(vim.deep_equal(comparison, view.comparison))
  callback(state, nil)
end
local refreshed = false
workflow.refresh(function(result, err)
  assert(result == state and err == nil)
  refreshed = true
end)
assert(refreshed and observes == 1, "refreshing a bound tab must record/resolve its current observation")

client.load_binding = function(root, binding_id, callback)
  loads = loads + 1
  assert(root == checkout and binding_id == "binding-one")
  callback(state, nil)
end
workflow.detach(tabpage, false)
workflow.attach(tabpage)
assert(binding.get(tabpage) == "binding-one" and observes == 2, "reattaching a mutable CodeDiff update must re-observe before rendering")

local replies = 0
composer.open = function(context, callback)
  assert(context.path == "src/lib.rs" and context.start_line == 1)
  callback("follow-up message")
end
client.add_message = function(root, binding_id, thread_id, message, callback)
  assert(root == checkout and binding_id == "binding-one" and thread_id == "thread-one")
  assert(message.body == "follow-up message" and message.author == "wolf")
  replies = replies + 1
  callback({ id = "message-two", thread_id = thread_id, author = message.author, body = message.body }, nil)
end
vim.api.nvim_win_set_cursor(0, { 1, 0 })
workflow.reply_at_cursor()
assert(replies == 1 and observes == 3, "replying must persist a message and refresh the binding")

state.agent_requests = { {
  id = "foreign-request",
  ordinal = 1,
  state = "awaiting_dispatch",
  recovery = "retry",
  runtime_binding_id = "binding-two",
  attempts = { { state = "pending", runtime_binding_id = "binding-two" } },
} }
assert(render.request_summary(state, "binding-one"):match("another runtime"), "status must retain and mark foreign request history")
assert(not binding.owns_request({ runtime_binding_id = "binding-one", attempts = {
  { runtime_binding_id = "binding-two" },
} }, "binding-one"), "the latest dispatch binding must own request actionability")
assert(binding.owns_request({ runtime_binding_id = "binding-two", attempts = {
  { runtime_binding_id = "binding-one" },
} }, "binding-one"), "a dispatch retried here must become actionable here")
local original_list_agents = client.list_agents
local original_retry_dispatch = client.retry_dispatch
local original_select = vim.ui.select
local agent_lists, retry_calls = 0, 0
local agent = { label = "worker", status = "idle", assignment = { pane_id = "pane" } }
client.list_agents = function(_, binding_id, callback)
  assert(binding_id == "binding-one")
  agent_lists = agent_lists + 1
  callback({ agent }, nil)
end
client.retry_dispatch = function(_, binding_id, request_id, _, callback)
  assert(binding_id == "binding-one" and request_id == "foreign-request")
  retry_calls = retry_calls + 1
  callback(nil, { code = "test_stop", message = "stop after proving rescue intent" })
end
vim.ui.select = function(items, _, callback) callback(items[1]) end
workflow.retry_dispatch()
assert(agent_lists == 1 and retry_calls == 1, "a reopened binding must be able to rescue a foreign pending dispatch")

for _, guarded_state in ipairs({ "dispatching", "unknown" }) do
  state.agent_requests[1].state = guarded_state == "dispatching" and "dispatching" or "awaiting_retry"
  state.agent_requests[1].attempts[1].state = guarded_state
  state.agent_requests[1].recovery = "inspect_before_retry"
  vim.ui.select = function(items, _, callback)
    if type(items[1]) == "table" then callback(items[1]); return end
    assert(type(items[1]) == "string", guarded_state .. " must require an explicit safety confirmation")
    callback(nil)
  end
  workflow.retry_dispatch()
end
assert(agent_lists == 1 and retry_calls == 1, "cancelling a risky foreign rescue must not dispatch")

state.agent_requests = {
  { id = "older", ordinal = 1, state = "awaiting_retry", recovery = "retry", runtime_binding_id = "binding-two", attempts = {
    { state = "blocked", runtime_binding_id = "binding-two" },
  } },
  { id = "newest", ordinal = 2, state = "returned", recovery = "retry", runtime_binding_id = "binding-two", attempts = {
    { state = "returned", runtime_binding_id = "binding-two" },
  } },
}
state.agent_requests[2].thread_ids = { "thread-one", "thread-two" }
state.agent_requests[2].answered_thread_ids = { "thread-one" }
local selected_request
client.retry_dispatch = function(_, _, request_id, _, callback)
  selected_request = request_id
  callback(nil, { code = "test_stop", message = "stop after proving recovery intent" })
end
for _, index in ipairs({ 1, 2 }) do
  vim.ui.select = function(items, select_options, callback)
    if select_options.prompt == "Recover an outstanding review request" then
      assert(#items == 2, "older failures and returned requests with missing answers must remain recoverable")
      callback(items[index])
    else
      callback(items[1])
    end
  end
  workflow.retry_dispatch()
  assert(selected_request == state.agent_requests[index].id, "recovery must use the explicitly selected request")
end
client.list_agents = original_list_agents
client.retry_dispatch = original_retry_dispatch
vim.ui.select = original_select
state.agent_requests = {}

-- Agent selection must not wait for review loading, and neither completion
-- order may dispatch until both inputs are ready.
do
  local saved_load, saved_agents = client.load_binding, client.list_agents
  local saved_request, saved_select = client.create_agent_request, vim.ui.select
  local dispatched = 0
  table.insert(state.threads, { id = "private-finding", status = "open", finding = { kind = "finding" } })
  for _, scenario in ipairs({ "agent_first", "state_first", "cancel", "load_error", "view_changed" }) do
    local loaded, listed, selected
    local before = dispatched
    client.load_binding = function(_, _, callback) loaded = callback end
    client.list_agents = function(_, _, callback) listed = callback end
    vim.ui.select = function(_, _, callback) selected = callback end
    client.create_agent_request = function(_, id, ids, assignment)
      assert(id == "binding-one" and #ids == 2 and ids[1] == "thread-one" and assignment == agent.assignment,
        "all open threads must reach the store's authoritative new-message selection")
      dispatched = dispatched + 1
    end
    workflow.request_agent()
    assert(loaded and listed, "both reads must start without waiting for each other")
    listed({ agent }, nil)
    assert(selected and dispatched == before, "picker must open before review load completes")
    if scenario == "state_first" then
      loaded(state, nil)
      assert(dispatched == before, "state alone must not dispatch")
      selected(agent)
    elseif scenario == "cancel" then
      selected(nil)
      loaded(state, nil)
    elseif scenario == "load_error" then
      selected(agent)
      loaded(nil, { message = "test load failure" })
    elseif scenario == "view_changed" then
      loaded(state, nil)
      binding.set(tabpage, "binding-other")
      selected(agent)
      binding.set(tabpage, "binding-one")
    else
      selected(agent)
      assert(dispatched == before, "selection alone must not dispatch")
      loaded(state, nil)
    end
    local expected = (scenario == "agent_first" or scenario == "state_first") and 1 or 0
    assert(dispatched == before + expected, scenario .. " dispatched incorrectly")
  end
  client.load_binding, client.list_agents = saved_load, saved_agents
  client.create_agent_request, vim.ui.select = saved_request, saved_select
  table.remove(state.threads)
end

local picker_options
_G.Snacks = { picker = { pick = function(options) picker_options = options end } }
require("herdr_review.picker").open(checkout, state, { jump = function() end, resolve = function() end })
assert(picker_options and picker_options.source == "review_threads")
assert(picker_options.win.preview.wo.wrap and picker_options.win.preview.wo.linebreak)
assert(picker_options.items[1].location == "lib.rs:1")
assert(picker_options.items[1].text:find("src/lib.rs", 1, true), "full paths must remain searchable")
assert(#picker_options.items == 1 and picker_options.items[1].preview.text:match("first message") == nil)
assert(picker_options.items[1].preview.text:match("change this"), "picker preview must render thread messages")
local general = {
  id = "general-finding", status = "open", anchor = vim.NIL, resolution = vim.NIL,
  finding = { key = "ownership", kind = "suggestion", title = "Clarify ownership", severity = vim.NIL,
    evidence = "Two owners", related_locations = { { path = "src/lib.rs", side = "target", start_line = 1, end_line = 2 } } },
  messages = { { author = "agent", body = "Discuss ownership across the components" } },
}
assert(location.target(general) == nil, "general findings must not acquire fake file locations")
require("herdr_review.picker").open(checkout, { threads = { thread, general } }, {
  jump = function() end, resolve = function() end,
})
assert(#picker_options.items == 2)
local general_item = picker_options.items[1]
assert(general_item.location == "General" and general_item.file == nil and general_item.pos == nil)
assert(general_item.preview.text:match("agent suggestion") and general_item.preview.text:match("Two owners"))
assert(general_item.preview.text:match("Related:"), "related evidence must remain separate from the comment anchor")
render.refresh(tabpage, view, { threads = { general } }, true)
assert(render.location(general, tabpage) == nil, "general findings have no gutter annotation")
require("herdr_review.picker").show(general)
assert(vim.bo.filetype == "markdown" and not vim.bo.modifiable)
vim.api.nvim_win_close(0, true)
_G.Snacks = nil

local ready, updating = 0, 0
codediff.observe({
  ready = function(candidate) assert(candidate == tabpage); ready = ready + 1 end,
  updating = function(candidate) assert(candidate == tabpage); updating = updating + 1 end,
})
local lifecycle = require("codediff.ui.lifecycle")
assert(lifecycle.create_session(tabpage, {}, {}) == "created")
assert(vim.wait(100, function() return ready == 1 end), "adapter must observe CodeDiff session creation")
lifecycle.update_diff_result(tabpage, nil)
assert(updating == 1, "adapter must suspend activity while CodeDiff updates")
lifecycle.update_diff_result(tabpage, {})
assert(vim.wait(100, function() return ready == 2 end), "adapter must reattach after CodeDiff commits an update")
assert(not codediff.same_view(view, assert(codediff.view(tabpage))), "CodeDiff commits must invalidate in-flight mutable observations")

local original_nvim_cmd = vim.api.nvim_cmd
local opened
vim.api.nvim_cmd = function(command) opened = command end
assert(codediff.open(state.observation.comparison))
assert(vim.deep_equal(opened.args, {
  "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
}), "captured observations must reopen as their exact immutable commits")
client.load_context = function(_, callback) callback({ state = state, checkout_root = checkout }, nil) end
opened = nil
workflow.open_review()
assert(opened and opened.cmd == "CodeDiff", "opening a saved review must be an explicit editor action")
do
  local branch = require("herdr_review.branch")
  local original_comparison = branch.comparison
  local fallbacks = 0
  branch.comparison = function(_, callback)
    fallbacks = fallbacks + 1
    callback({ base = { kind = "commit", oid = "c" }, target = { kind = "working_tree" } })
  end
  workflow.open_review({ branch_fallback = true })
  assert(fallbacks == 0 and opened.args[2] == string.rep("b", 40), "PR diffs must retain captured endpoints without fetching")
  client.load_context = function(_, callback) callback(nil, nil) end
  workflow.open_review({ branch_fallback = true })
  assert(fallbacks == 1 and opened.args[1] == "c")
  workflow.open_review()
  assert(fallbacks == 1, "ReviewOpen must not silently become a branch diff")
  client.load_context = function(_, callback) callback(nil, "context lookup failed") end
  workflow.open_review({ branch_fallback = true })
  assert(fallbacks == 1, "context errors must not silently select an unrelated comparison")
  branch.comparison = original_comparison
end
vim.api.nvim_cmd = original_nvim_cmd

do
  local original_system = vim.system
  local branch = require("herdr_review.branch")
  for _, case in ipairs({
    { remotes = "origin\nupstream", configured = "origin", expected = "upstream" },
    { remotes = "origin\nteam", configured = "team", expected = "team" },
    { remotes = "origin", configured = "", expected = "origin" },
    { remotes = "team", configured = "", expected = "team" },
    { remotes = "one\ntwo", configured = "", ambiguous = true },
    { remotes = "upstream", configured = "", expected = "upstream", fetch_error = true },
  }) do
    local calls, result, error_message, done = {}, nil, nil, false
    vim.system = function(command, options, callback)
      assert(options.timeout == 30000 and options.env.GIT_TERMINAL_PROMPT == "0")
      local args = vim.list_slice(command, 4)
      calls[#calls + 1] = args
      local output, code = "", 0
      if args[1] == "remote" then output = case.remotes
      elseif args[1] == "symbolic-ref" then output = "feature"
      elseif args[1] == "config" then output = case.configured; code = output == "" and 1 or 0
      elseif args[1] == "ls-remote" then
        assert(args[3] == case.expected)
        output = "ref: refs/heads/trunk\tHEAD\n" .. string.rep("a", 40) .. "\tHEAD"
      elseif args[1] == "fetch" then
        assert(vim.deep_equal(args, { "fetch", "--no-tags", "--no-write-fetch-head", case.expected,
          "+refs/heads/trunk:refs/remotes/" .. case.expected .. "/trunk" }))
        code = case.fetch_error and 1 or 0
      elseif args[1] == "merge-base" then
        assert(args[2] == "refs/remotes/" .. case.expected .. "/trunk" and args[3] == "HEAD")
        output = string.rep("a", 40)
      else error("unexpected git command") end
      callback({ code = code, stdout = output, stderr = code == 1 and "test failure" or "" })
    end
    branch.comparison(checkout, function(comparison, err)
      result, error_message, done = comparison, err, true
    end)
    assert(vim.wait(500, function() return done end))
    if case.ambiguous or case.fetch_error then
      assert(not result and error_message, "unsafe or failed resolution must not fall back to main")
    else
      assert(result.base.oid == string.rep("a", 40) and result.target.kind == "working_tree")
    end
  end
  vim.system = original_system
end

-- A manually opened comparison adopts the shell-created binding only when it
-- represents the captured PR. Discovery must never observe an arbitrary diff.
workflow.detach(tabpage, true)
local previous_revision = session.modified_revision
client.load_context = function(_, callback) callback({ state = state, checkout_root = checkout }, nil) end
session.modified_revision = "cccccccccccccccccccccccccccccccccccccccc"
workflow.attach(tabpage)
assert(binding.get(tabpage) == nil, "unrelated comparisons must not inherit a PR binding")
session.modified_revision = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
workflow.attach(tabpage)
assert(binding.get(tabpage) == "binding-one", "matching comparisons must discover the shell-created binding")
do
  local saved_view, saved_buffer_view, saved_open, saved_jump = codediff.view, codediff.view_for_buffer, codediff.open, codediff.jump
  codediff.view, codediff.view_for_buffer = function() end, function() end
  local requested, jumped = false, false
  codediff.open = function(comparison)
    assert(vim.deep_equal(comparison, state.observation.comparison))
    requested = true
    return true
  end
  codediff.jump = function(candidate, target, _, callback)
    assert(candidate == tabpage and target.path == "src/lib.rs")
    jumped = true
    callback(nil)
  end
  _G.Snacks = { picker = { pick = function(opts) picker_options = opts end } }
  workflow.pick_threads()
  picker_options.confirm({ close = function() end }, picker_options.items[1])
  assert(vim.wait(100, function() return requested end), "plain-buffer inline selection must open the captured comparison")
  assert(not jumped, "navigation must wait for CodeDiff readiness")
  codediff.view, codediff.view_for_buffer = saved_view, saved_buffer_view
  workflow.attach(tabpage)
  assert(vim.wait(100, function() return jumped end), "ready comparison must navigate to the selected thread side")
  codediff.open, codediff.jump = saved_open, saved_jump
  _G.Snacks = nil
end
session.modified_revision = previous_revision

-- Binding freshness outlives dispatch and retries transient read failures.
do
  local saved_defer, saved_render = vim.defer_fn, render.refresh
  local queued, rendered, reads, fail = {}, 0, 0, false
  vim.defer_fn = function(callback) table.insert(queued, callback) end
  render.refresh = function() rendered = rendered + 1 end
  client.load_binding = function(_, _, callback)
    reads = reads + 1
    if fail then callback(nil, { message = "temporary read failure" }) else callback(vim.deepcopy(state), nil) end
  end
  client.observe_binding = function(_, _, _, callback) callback(vim.deepcopy(state), nil) end
  package.loaded["herdr_review.workflow"] = nil
  local independent = require("herdr_review.workflow")
  independent.setup({ poll_interval_ms = 10 })
  binding.set(tabpage, state.binding.id)
  independent.attach(tabpage, true)
  assert(#queued == 1 and reads == 1, "attaching a binding with no running dispatch must start one refresh loop")
  table.insert(state.threads[1].messages, { id = "external-answer", author = "agent", body = "Saved independently" })
  table.remove(queued, 1)()
  assert(reads == 2 and rendered >= 2 and #queued == 1, "external answers must appear without a dispatch watcher")
  fail = true
  table.remove(queued, 1)()
  assert(#queued == 1, "transient errors must keep the binding refresh loop alive")
  fail = false
  table.remove(queued, 1)()
  local delayed
  client.load_binding = function(_, _, callback) reads = reads + 1; delayed = callback end
  local prior = reads
  independent.status()
  independent.status()
  assert(reads == prior + 1, "simultaneous readers of one binding must share the same load")
  local before = reads
  independent.detach(tabpage, true)
  delayed(vim.deepcopy(state), nil)
  assert(binding.get(tabpage) == nil, "an in-flight load must not reattach a closed binding")
  table.remove(queued, 1)()
  assert(reads == before and #queued == 0, "closing the last attached view must stop refreshing")
  table.remove(state.threads[1].messages)
  vim.defer_fn, render.refresh = saved_defer, saved_render
  package.loaded["herdr_review.workflow"] = workflow
end

composer.open = original_compose
client.load_binding = original_load
client.load_context = original_context
client.create_thread = original_create
client.add_message = original_add_message
client.observe_binding = original_observe
workflow.detach(tabpage, true)
assert(binding.get(tabpage) == nil and released_keymaps > 0)

assert(location.line(location.target(thread)) == 1)
require("herdr_review").setup()
assert(vim.fn.exists(":ReviewThread") == 2 and vim.fn.exists(":ReviewSend") == 2)
assert(vim.fn.exists(":ReviewReply") == 2)
assert(vim.fn.exists(":ReviewRetryDispatch") == 2, "ordinary CodeDiff setup must install the activity commands without a binding")
print("herdr_review smoke: ok")
