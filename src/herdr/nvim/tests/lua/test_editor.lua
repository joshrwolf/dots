local T, child = require("support").child_set()
local eq = MiniTest.expect.equality
local function setup()
  child.lua_func(function()
    context = require("support").context()
    fake = require("fake_host").new(context)
    endpoint = require("dots.herdr.endpoint").new(context.server, fake.listen())
    attachment = require("dots.herdr.attachment").new(fake.host, endpoint, function() end)
    attachment.start()
    fake.publications[1].commit()
    fake.publications[1].deliver()
    dispatch = require("dots.herdr.bridge").new(endpoint)
    path = test_root .. "/quoted 'file\" λ.txt"
    vim.fn.writefile({ "first", "sécónd", "third" }, path)
    original = vim.api.nvim_get_current_buf()
    vim.api.nvim_buf_set_lines(original, 0, -1, false, { "unsaved original" })
    function open(overrides)
      local operation = vim.tbl_extend("force", {
        method = "open",
        not_after_ms = require("dots.herdr.protocol").now_ms() + 2000,
        target = { path = path, line = 2, column = 2 },
      }, overrides or {})
      return vim.json.decode(dispatch(vim.json.encode({ protocol = 1, context = context, operation = operation })))
    end
  end)
end

T["opens quoted Unicode paths and preserves an unsaved buffer"] = function()
  setup()
  eq(
    child.lua_func(function()
      local result = open()
      return {
        result.status,
        result.target.column,
        vim.bo.buflisted,
        vim.api.nvim_win_get_cursor(0),
        vim.bo[original].modified,
        vim.api.nvim_buf_get_lines(original, 0, -1, false),
      }
    end),
    { "opened", 2, true, { 2, 1 }, true, { "unsaved original" } }
  )
end

T["invalid positions never select the target buffer"] = function()
  setup()
  eq(
    child.lua_func(function()
      local result = open({ target = { path = path, line = 999, column = 1 } })
      return { result.status, result.error.code, vim.api.nvim_get_current_buf() == original }
    end),
    { "rejected", "invalid_position", true }
  )
end

T["byte positions cannot land inside a UTF-8 sequence"] = function()
  setup()
  eq(
    child.lua_func(function()
      return open({ target = { path = path, line = 2, column = 3 } }).error.code
    end),
    "invalid_position"
  )
end

T["dirty buffers cannot be abandoned when hidden is disabled"] = function()
  setup()
  eq(
    child.lua_func(function()
      vim.o.hidden = false
      local result = open()
      return { result.status, result.error.code, vim.api.nvim_get_current_buf() == original }
    end),
    { "rejected", "dirty_buffer", true }
  )
end

T["BufRead mutation is revalidated before presentation"] = function()
  setup()
  eq(
    child.lua_func(function()
      vim.api.nvim_create_autocmd("BufReadPost", {
        once = true,
        callback = function()
          endpoint.close()
        end,
      })
      local result = open()
      return { result.status, result.error.code, vim.api.nvim_get_current_buf() == original }
    end),
    { "rejected", "not_attached", true }
  )
end

T["BufEnter failure is partial, not a false rejection"] = function()
  setup()
  eq(
    child.lua_func(function()
      vim.fn.bufload(vim.fn.bufadd(path)) -- isolate presentation from BufRead hooks
      vim.api.nvim_create_autocmd("BufEnter", {
        once = true,
        callback = function()
          error("synthetic presentation failure")
        end,
      })
      local result = open()
      return { result.status, result.error.code }
    end),
    { "partial", "presentation_failed" }
  )
end

T["navigation performs no synchronous Herdr calls"] = function()
  setup()
  eq(
    child.lua_func(function()
      fake.host.current = function()
        error("navigation must not query Herdr")
      end
      return open().status
    end),
    "opened"
  )
end

T["withdrawn pane advertisements do not prevent local navigation"] = function()
  setup()
  eq(
    child.lua_func(function()
      attachment.stop()
      local result = open()
      return { attachment.phase(), fake.pane.registration == nil, result.status }
    end),
    { "stopped", true, "opened" }
  )
end

T["metadata alone cannot authorize a closed endpoint"] = function()
  setup()
  eq(
    child.lua_func(function()
      endpoint.close()
      return open().error.code
    end),
    "not_attached"
  )
end
T["swap conflicts stay rejected across retries with another live editor"] = function()
  setup()
  local holder = MiniTest.new_child_neovim()
  MiniTest.finally(holder.stop)
  holder.start({ "-u", vim.env.HERDR_NVIM_TESTS .. "/lua/child.lua", "-i", "NONE" })
  holder.lua_func(function(file)
    vim.cmd("set updatecount& swapfile&")
    vim.cmd("edit " .. vim.fn.fnameescape(file))
    vim.api.nvim_buf_set_lines(0, 0, -1, false, { "unsaved in the other editor" })
    vim.cmd("preserve")
    assert(vim.uv.fs_stat(vim.fn.swapname(vim.api.nvim_get_current_buf())))
  end, child.lua_get("path"))
  eq(
    child.lua_func(function()
      local first, second = open(), open()
      return {
        first.status,
        first.error and first.error.code or "none",
        second.status,
        second.error and second.error.code or "none",
        vim.api.nvim_get_current_buf() == original,
      }
    end),
    { "rejected", "swap_conflict", "rejected", "swap_conflict", true }
  )
  eq(
    holder.lua_func(function()
      return { vim.bo.modified, vim.api.nvim_buf_get_lines(0, 0, -1, false) }
    end),
    { true, { "unsaved in the other editor" } }
  )
end

T["recorded swap conflicts cannot be bypassed by an already-loaded buffer"] = function()
  setup()
  eq(
    child.lua_func(function()
      local buffer = vim.fn.bufadd(path)
      vim.fn.bufload(buffer)
      vim.b[buffer].dots_herdr_swap_conflict = true
      local result = open()
      return { result.status, result.error and result.error.code or "none", vim.api.nvim_get_current_buf() == original }
    end),
    { "rejected", "swap_conflict", true }
  )
end

T["expired requests do not even create a target buffer"] = function()
  setup()
  eq(
    child.lua_func(function()
      local before = vim.api.nvim_list_bufs()
      local reply = open({ not_after_ms = 1 })
      return {
        reply.error.code,
        vim.deep_equal(before, vim.api.nvim_list_bufs()),
        vim.api.nvim_get_current_buf() == original,
      }
    end),
    { "expired", true, true }
  )
end

T["expiry during BufRead prevents presentation"] = function()
  setup()
  eq(
    child.lua_func(function()
      local read = false
      vim.api.nvim_create_autocmd("BufReadPost", {
        once = true,
        callback = function()
          read = true
          vim.wait(150)
        end,
      })
      local reply = open({ not_after_ms = require("dots.herdr.protocol").now_ms() + 75 })
      return { reply.status, reply.error.code, read, vim.api.nvim_get_current_buf() == original }
    end),
    { "rejected", "expired", true, true }
  )
end

T["buffer renaming during presentation cannot produce an echoed success"] = function()
  setup()
  eq(
    child.lua_func(function()
      vim.fn.bufload(vim.fn.bufadd(path))
      vim.api.nvim_create_autocmd("BufEnter", {
        once = true,
        callback = function()
          vim.api.nvim_buf_set_name(0, test_root .. "/renamed.txt")
        end,
      })
      local reply = open()
      return { reply.status, reply.error.code }
    end),
    { "partial", "presentation_failed" }
  )
end

T["special windows"] =
  MiniTest.new_set({ parametrize = { { "nofile" }, { "help" }, { "quickfix" }, { "terminal" }, { "floating" } } })
T["special windows"]["route to an existing ordinary window"] = function(kind)
  setup()
  eq(
    child.lua_func(function(kind)
      local ordinary = vim.api.nvim_get_current_win()
      local special = vim.api.nvim_create_buf(false, true)
      if kind == "floating" then
        vim.api.nvim_open_win(special, true, { relative = "editor", row = 1, col = 1, width = 20, height = 4 })
      else
        vim.cmd("vnew")
        vim.api.nvim_win_set_buf(0, special)
        if kind == "terminal" then
          vim.api.nvim_open_term(special, {})
        else
          vim.bo[special].buftype = kind
        end
      end
      local reply = open()
      return { reply.status, vim.api.nvim_get_current_win() == ordinary, vim.bo.buflisted, vim.bo[original].modified }
    end, kind),
    { "opened", true, true, true }
  )
end

T["special-window routing still protects a dirty destination"] = function()
  setup()
  eq(
    child.lua_func(function()
      vim.cmd("vnew")
      vim.bo.buftype = "nofile"
      local current = vim.api.nvim_get_current_win()
      vim.o.hidden = false
      local reply = open()
      return { reply.error.code, vim.api.nvim_get_current_win() == current, vim.bo[original].modified }
    end),
    { "dirty_buffer", true, true }
  )
end

T["no suitable window is an explicit rejection"] = function()
  setup()
  eq(
    child.lua_func(function()
      vim.bo.buftype = "nofile"
      local reply = open()
      return { reply.status, reply.error.code, vim.api.nvim_get_current_buf() == original }
    end),
    { "rejected", "unsupported_window", true }
  )
end
return T
