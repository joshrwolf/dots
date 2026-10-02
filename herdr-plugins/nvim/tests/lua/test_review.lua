local T, child = require("support").child_set()
local eq = MiniTest.expect.equality

T["setup is passive and preserves the recent-files mapping"] = function()
  eq(
    child.lua_func(function()
      vim.g.mapleader = " "
      vim.keymap.set("n", "<leader>fr", "<cmd>echo 'recent'<cr>", { desc = "Recent files" })
      require("dots.review").setup()
      return {
        vim.fn.exists(":ReviewThreads"),
        vim.fn.maparg("<leader>fr", "n", false, true).desc,
        vim.fn.maparg("<leader>fR", "n", false, true).desc,
      }
    end),
    { 2, "Recent files", "Find saved review threads" }
  )
end

local function prepare()
  child.lua_func(function()
    notices, writes, inputs = {}, {}, {}
    context = {
      checkout_root = test_root,
      state = {
        binding = { id = "binding" },
        threads = {
          {
            id = "thread",
            status = "open",
            anchor = vim.NIL,
            messages = { { body = "Question\nMore café 👩‍💻", author = "reviewer" } },
          },
        },
      },
    }
    package.loaded["dots.review.client"] = {
      load_context = function(_, callback)
        loaded = callback
      end,
      load_binding = function(_, _, callback)
        callback(context.state)
      end,
      add_message = function(root, binding, thread, message, callback)
        table.insert(writes, { root = root, binding = binding, thread = thread, message = message })
        saved = callback
      end,
      resolve_thread = function(_, _, _, callback)
        saved = callback
      end,
    }
    vim.notify = function(message)
      table.insert(notices, message)
    end
    vim.ui.select = function(items, _, callback)
      callback(items[1])
    end
    vim.ui.input = function(opts, callback)
      table.insert(inputs, opts)
      entered = callback
    end
    review = require("dots.review")
    review.setup()
    review.threads()
  end)
end

T["general discussions preserve the editing buffer without fabricating a location"] = function()
  prepare()
  eq(
    child.lua_func(function()
      local original = vim.api.nvim_get_current_buf()
      vim.api.nvim_buf_set_lines(original, 0, -1, false, { "unsaved work" })
      loaded(context)
      local text = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n")
      return {
        text:find("General discussion", 1, true) ~= nil,
        text:find("More café 👩‍💻", 1, true) ~= nil,
        vim.bo.modifiable,
        vim.bo.buftype,
        vim.bo[original].modified,
        vim.api.nvim_buf_get_lines(original, 0, -1, false),
      }
    end),
    { true, true, false, "nofile", true, { "unsaved work" } }
  )
end

T["finding evidence can contain multiple lines"] = function()
  prepare()
  eq(
    child.lua_func(function()
      context.state.threads[1].finding = { title = "Finding", evidence = "first line\nsecond line" }
      loaded(context)
      local text = table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n")
      return text:find("first line\nsecond line", 1, true) ~= nil
    end),
    true
  )
end

T["late context responses do not steal focus"] = function()
  prepare()
  eq(
    child.lua_func(function()
      vim.cmd("new")
      local buf = vim.api.nvim_get_current_buf()
      local windows = #vim.api.nvim_list_wins()
      loaded(context)
      return { vim.api.nvim_get_current_buf() == buf, #vim.api.nvim_list_wins() == windows }
    end),
    { true, true }
  )
end

T["closing a discussion before input returns cannot submit a reply"] = function()
  prepare()
  eq(
    child.lua_func(function()
      loaded(context)
      review.reply()
      vim.api.nvim_buf_delete(0, { force = true })
      entered("do not save")
      return #writes
    end),
    0
  )
end

T["uncertain replies are not repeated and retain the draft for inspection"] = function()
  prepare()
  eq(
    child.lua_func(function()
      loaded(context)
      review.reply()
      entered("draft café 👩‍💻")
      saved(nil, { code = "timeout", message = "delivery uncertain" })
      review.reply()
      local blocked = #inputs == 1 and #writes == 1
      review.refresh()
      review.reply()
      local default = inputs[2].default
      entered(nil)
      return { blocked, default, writes[1].binding, writes[1].thread, writes[1].message.author, #writes }
    end),
    { true, "draft café 👩‍💻", "binding", "thread", "reviewer", 1 }
  )
end

return T
