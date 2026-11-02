-- codediff.api: exact-revision explorers with caller-supplied files, buffer
-- identity, landing on a line after content loads, file-row decorations,
-- CodeDiffFileShown, and native quickfix jumps into an explorer tab.
local h = require("tests.support")

local function numbered(prefix, count)
  local lines = {}
  for i = 1, count do
    lines[i] = prefix .. " " .. i
  end
  return lines
end

describe("codediff.api", function()
  local api, lifecycle, repo, base, head, saved_cwd

  before_each(function()
    h.ensure_plugin_loaded()
    require("codediff").setup({ explorer = { view_mode = "list" } })
    api = require("codediff.api")
    lifecycle = require("codediff.ui.lifecycle")
    saved_cwd = vim.fn.getcwd()
    repo = h.create_temp_git_repo()
    repo.write_file("f.txt", numbered("line", 30))
    repo.write_file("gone.txt", numbered("gone", 30))
    repo.write_file("other.txt", { "other" })
    base = repo.commit("base")
    local changed = numbered("line", 30)
    changed[25] = "CHANGED 25"
    repo.write_file("f.txt", changed)
    repo.write_file("added.txt", numbered("added", 30))
    vim.fn.delete(repo.path("gone.txt"))
    repo.write_file("other.txt", { "other changed" })
    head = repo.commit("head")
    -- A dirty working tree must not leak into a revision explorer.
    repo.write_file("dirty.txt", { "untracked" })
    vim.cmd("cd " .. vim.fn.fnameescape(repo.dir))
  end)

  after_each(function()
    pcall(vim.cmd, "cd " .. vim.fn.fnameescape(saved_cwd))
    lifecycle.cleanup_all()
    while vim.fn.tabpagenr("$") > 1 do
      vim.cmd("tabclose!")
    end
    vim.fn.setqflist({}, "f")
    repo.cleanup()
  end)

  local files = {
    { path = "added.txt", status = "A" },
    { path = "f.txt", status = "M" },
    { path = "gone.txt", status = "D" },
  }

  local function open()
    local tab = assert(api.open({ root = repo.dir, base = base, head = head, files = files }))
    assert.is_true(
      vim.wait(5000, function()
        return require("codediff.ui.refresh").shown(tab)
      end, 10),
      "explorer should show its first file"
    )
    return tab
  end

  local function show(tab, target)
    local landed
    assert.is_true(api.show(tab, target, function(win, buf)
      landed = { win = win, buf = buf, line = vim.api.nvim_win_get_cursor(win)[1], count = vim.api.nvim_buf_line_count(buf) }
    end))
    assert.is_true(
      vim.wait(5000, function()
        return landed ~= nil
      end, 10),
      "show should land on " .. target.path
    )
    return landed
  end

  local function explorer_paths(tab)
    local panel = lifecycle.get_panel(tab)
    local paths = {}
    for _, file in ipairs(require("codediff.ui.explorer").get_all_files(panel.view.tree)) do
      paths[#paths + 1] = file.data.path
    end
    table.sort(paths)
    return paths
  end

  it("opens exact revisions over the supplied files only, across refreshes", function()
    local tab = open()
    assert.same({ root = repo.dir, base = base, head = head }, api.revisions(tab))
    assert.same({ "added.txt", "f.txt", "gone.txt" }, explorer_paths(tab))
    local refreshed = false
    require("codediff.ui.refresh").request(tab, { full = true }, function()
      refreshed = true
    end)
    assert.is_true(vim.wait(5000, function()
      return refreshed
    end, 20))
    assert.same({ "added.txt", "f.txt", "gone.txt" }, explorer_paths(tab), "Git status must not replace the file set")
  end)

  it("refuses incomplete open requests", function()
    local _, err = api.open({ root = repo.dir, base = "HEAD", head = head, files = files })
    assert.matches("base must be a full commit id", err)
    _, err = api.open({ root = repo.dir, base = base, head = head, files = {} })
    assert.matches("at least one file", err)
    _, err = api.open({ root = repo.dir, base = base, head = head, files = { { path = "f.txt", status = "R" } } })
    assert.matches("files%[1%]", err)
    assert.equals(1, vim.fn.tabpagenr("$"))
  end)

  it("identifies a tab's revision buffers and creates missing ones unloaded", function()
    local tab = open()
    local original, modified = lifecycle.get_buffers(tab)
    local path = api.buffer(modified, tab).path
    assert.same({ path = path, revision = head, side = "modified" }, api.buffer(modified, tab))
    if path == "f.txt" then
      assert.same({ path = "f.txt", revision = base, side = "original" }, api.buffer(original, tab))
    end
    assert.is_nil(api.buffer(vim.api.nvim_create_buf(false, true), tab))

    local buf = assert(api.revision_buffer(tab, "gone.txt", "original"))
    assert.same({ path = "gone.txt", revision = base, side = "original" }, api.buffer(buf, tab))
    assert.is_false(vim.api.nvim_buf_is_loaded(buf))
    local _, err = api.revision_buffer(tab, "gone.txt", "left")
    assert.matches("has no left revision", err)
  end)

  it("lands on a line of either side once the file is rendered", function()
    local tab = open()
    local shown = {}
    vim.api.nvim_create_autocmd("User", {
      pattern = "CodeDiffFileShown",
      callback = function(event)
        shown[#shown + 1] = event.data.path
      end,
    })
    local landed = show(tab, { path = "f.txt", side = "modified", line = 25 })
    assert.same({ path = "f.txt", revision = head, side = "modified" }, api.buffer(landed.buf, tab))
    assert.equals(25, landed.line)
    assert.equals(landed.win, vim.api.nvim_get_current_win())
    assert.equals("f.txt", shown[#shown])

    -- The other side of the file already shown lands without a reload.
    local count = #shown
    landed = show(tab, { path = "f.txt", side = "original", line = 7 })
    assert.equals(base, api.buffer(landed.buf, tab).revision)
    assert.equals(7, landed.line)
    assert.equals(count, #shown)
  end)

  it("waits for slow one-sided content before calling back", function()
    local tab = open()
    local git = require("codediff.core.git")
    local get_file_content = git.get_file_content
    git.get_file_content = function(revision, root, file, callback)
      return get_file_content(revision, root, file, function(err, lines)
        if file == "added.txt" or file == "gone.txt" then
          vim.defer_fn(function()
            callback(err, lines)
          end, 300)
        else
          callback(err, lines)
        end
      end)
    end
    local ok, failure = pcall(function()
      local at_event
      vim.api.nvim_create_autocmd("User", {
        pattern = "CodeDiffFileShown",
        callback = function(event)
          if event.data.path == "added.txt" then
            at_event = vim.api.nvim_buf_line_count(event.data.modified_buf)
          end
        end,
      })
      -- Move off added.txt first if the explorer opened on it.
      show(tab, { path = "f.txt", side = "modified", line = 1 })
      local landed = show(tab, { path = "added.txt", side = "modified", line = 20 })
      assert.equals(30, landed.count)
      assert.equals(20, landed.line)
      assert.equals(30, at_event, "CodeDiffFileShown must wait for the content")
      landed = show(tab, { path = "gone.txt", side = "original", line = 12 })
      assert.equals(30, landed.count)
      assert.equals(12, landed.line)
      assert.equals(base, api.buffer(landed.buf, tab).revision)
    end)
    git.get_file_content = get_file_content
    assert(ok, failure)
  end)

  it("refuses targets the diff cannot show", function()
    local tab = open()
    local _, err = api.show(tab, { path = "other.txt", side = "modified", line = 1 })
    assert.matches("not in this diff", err)
    _, err = api.show(tab, { path = "added.txt", side = "original", line = 1 })
    assert.matches("has no original side", err)
    _, err = api.show(tab, { path = "gone.txt", side = "modified", line = 1 })
    assert.matches("has no modified side", err)
    _, err = api.show(vim.api.nvim_create_buf(false, true) + 1000, { path = "f.txt", side = "modified", line = 1 })
    assert.matches("no CodeDiff explorer", err)
  end)

  it("draws and clears caller decorations on file rows", function()
    local tab = open()
    local explorer = lifecycle.get_panel(tab).view
    local function row(path)
      for _, line in ipairs(vim.api.nvim_buf_get_lines(explorer.bufnr, 0, -1, false)) do
        if line:find(path, 1, true) then
          return line
        end
      end
    end
    assert.is_true(api.set_decorations(tab, { ["f.txt"] = { text = "●2", hl = "DiagnosticWarn" } }))
    assert.matches("●2", row("f.txt"))
    assert.is_nil(row("added.txt"):find("●", 1, true))
    assert.is_true(api.set_decorations(tab, {}))
    assert.is_nil(row("f.txt"):find("●", 1, true))
  end)

  it("follows quickfix jumps into its revision buffers", function()
    local tab = open()
    show(tab, { path = "f.txt", side = "modified", line = 1 })
    local added = assert(api.revision_buffer(tab, "added.txt", "modified"))
    local gone = assert(api.revision_buffer(tab, "gone.txt", "original"))
    vim.fn.setqflist({
      { bufnr = added, lnum = 15, text = "added" },
      { bufnr = gone, lnum = 10, text = "gone" },
    })
    local session = lifecycle.get_session(tab)
    local function at(path, side, line)
      return vim.wait(5000, function()
        local where = api.buffer(vim.api.nvim_get_current_buf(), tab)
        return where ~= nil
          and where.path == path
          and where.side == side
          and vim.fn.line(".") == line
          and vim.api.nvim_buf_line_count(0) > 1
          and require("codediff.ui.refresh").shown(tab)
      end, 10) or vim.inspect({
        api.buffer(vim.api.nvim_get_current_buf(), tab),
        vim.fn.line("."),
        vim.api.nvim_buf_line_count(0),
        require("codediff.ui.refresh").shown(tab),
        vim.api.nvim_buf_get_name(0),
      })
    end

    -- From a diff pane, :cc reuses the pane; CodeDiff takes the file over.
    vim.api.nvim_set_current_win(session.modified_win)
    vim.cmd("cc 1")
    assert.equals(true, at("added.txt", "modified", 15), "cc 1 should land on added.txt:15")
    assert.equals("added.txt", lifecycle.get_panel(tab).data.current_selection.path)

    -- From the quickfix window, the jump splits; the split is closed again.
    vim.cmd("copen")
    vim.cmd("cc 2")
    assert.equals(true, at("gone.txt", "original", 10), "cc 2 should land on gone.txt:10")
    -- A deleted file shows one pane: that pane, the explorer and the quickfix window.
    assert.equals(3, #vim.api.nvim_tabpage_list_wins(tab), "the split the jump opened is closed")

    -- An old-side entry of a two-sided file lands in the original pane. CodeDiff
    -- wipes revision buffers it stops showing, so callers rebuild their lists.
    vim.fn.setqflist({ { bufnr = assert(api.revision_buffer(tab, "f.txt", "original")), lnum = 5, text = "f old" } })
    vim.cmd("copen")
    vim.cmd("cc 1")
    assert.equals(true, at("f.txt", "original", 5), "the old-side entry should land on f.txt:5 (original)")
    assert.equals(session.original_win, vim.api.nvim_get_current_win())
    assert.is_not_nil(lifecycle.get_session(tab), "the session must survive the jumps")
  end)
end)
