local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local nav = require("dots.nav")

return {
  ["editors take every direction"] = function()
    for _, program in ipairs({ "nvim", "NVIM", "vim", "vi", "vim.basic", "nvimdiff", "vim.exe" }) do
      for _, direction in ipairs({ "left", "down", "up", "right" }) do
        expect.equal(nav.decide(direction, program), "forward")
      end
    end
  end,

  ["look-alike names are not editors"] = function()
    for _, program in ipairs({ "vimx", "nvim-qt", "vim.", "gvim", "vim.a-b" }) do
      expect.equal(nav.is_vim(program), false)
    end
  end,

  ["pickers take only up and down"] = function()
    for _, program in ipairs({ "fzf", "dots-pick" }) do
      expect.equal(nav.decide("up", program), "forward")
      expect.equal(nav.decide("down", program), "forward")
      expect.equal(nav.decide("left", program), "focus")
      expect.equal(nav.decide("right", program), "focus")
    end
  end,

  ["shells and unknown programs move focus"] = function()
    expect.equal(nav.decide("left", "zsh"), "focus")
    expect.equal(nav.decide("up", nil), "focus")
  end,

  ["binds one function per direction"] = function(t)
    local _, bindings = fake_rex.registered(t, nav)
    local keys = {}
    for _, binding in ipairs(bindings) do
      keys[#keys + 1] = binding.key
      expect.equal(type(binding.target), "function")
    end
    table.sort(keys)
    expect.equal(keys, { "ctrl+h", "ctrl+j", "ctrl+k", "ctrl+l" })
  end,

  ["a key moves focus unless the program takes it"] = function(t)
    local _, bindings = fake_rex.registered(t, nav)
    local left ---@type rex.KeyFn
    for _, binding in ipairs(bindings) do
      if binding.key == "ctrl+h" then
        left = binding.target --[[@as rex.KeyFn]]
      end
    end
    local fake = fake_rex.install(t, {
      sessions = {
        {
          session_id = "s",
          label = "s",
          terminals = {
            editor = { process = { foreground = { name = "nvim" } } },
            shell = { process = { foreground = { name = "zsh" } } },
          },
        },
      },
    })
    local key = { keybinding = "ctrl+h" }
    expect.equal(left({ session_id = "s", block_id = "editor" }, key), false)
    expect.equal(fake.only("session.focus_direction"), {})
    expect.equal(left({ session_id = "s", block_id = "shell" }, key), true)
    expect.equal(
      fake.only("session.focus_direction"),
      { { method = "session.focus_direction", args = { direction = "left" } } }
    )
  end,
}
