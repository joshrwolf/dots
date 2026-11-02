-- Revision buffers keep tools that key on 'filetype' away, while diagnostics a
-- caller sets on them (review threads, for example) display.
local h = require("tests.support.e2e")

describe("Virtual buffer diagnostics integration", function()
  local repo, screen

  before_each(function()
    repo = h.repo()
    repo.write_file("sample.lua", { "local value = 'working'", "return value" })
    screen = h.screen("polling", "side-by-side")
  end)

  after_each(function()
    h.close(screen, repo)
    screen, repo = nil, nil
  end)

  it("attaches no filetype tools and displays caller diagnostics", function()
    h.open(screen, repo, "CodeDiff file HEAD", "sample.lua", "sample.lua")
    h.expect_text(screen, "original", "local value = 'base'")
    h.expect_text(screen, "modified", "local value = 'working'")
    local state = screen:exec([[
      local session = refresh_session()
      local buf = session.original_bufnr
      local ns = vim.api.nvim_create_namespace("caller")
      vim.diagnostic.config({ virtual_text = true }, ns)
      vim.diagnostic.set(ns, buf, { { lnum = 0, col = 0, message = "caller note", severity = vim.diagnostic.severity.WARN } })
      local shown = vim.api.nvim_buf_get_extmarks(buf, -1, 0, -1, { details = true })
      local note = false
      for _, mark in ipairs(shown) do
        for _, chunk in ipairs(mark[4].virt_text or {}) do
          note = note or chunk[1]:find("caller note", 1, true) ~= nil
        end
      end
      return {
        virtual = vim.api.nvim_buf_get_name(buf):match('^codediff://') ~= nil,
        filetype = vim.bo[buf].filetype,
        clients = #vim.lsp.get_clients({ bufnr = buf }),
        enabled = vim.diagnostic.is_enabled({ bufnr = buf }),
        note = note,
      }
    ]])
    assert.is_true(state.virtual, "original pane must be a real revision buffer")
    assert.equals("", state.filetype, "revision buffers must not set 'filetype'")
    assert.equals(0, state.clients, "no LSP client may attach to a revision buffer")
    assert.is_true(state.enabled, "revision buffers must not disable diagnostics")
    assert.is_true(state.note, "a caller's diagnostic must display")
  end)
end)
