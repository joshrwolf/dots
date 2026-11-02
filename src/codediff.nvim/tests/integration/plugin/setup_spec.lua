-- Public module loading and setup.

describe("plugin setup", function()
  local original_loaded = {}

  -- Simulate fresh module loading.
  local function reset_module_cache()
    for name, _ in pairs(package.loaded) do
      if name:match("^codediff") then
        original_loaded[name] = package.loaded[name]
        package.loaded[name] = nil
      end
    end
  end

  after_each(function()
    for name, mod in pairs(original_loaded) do
      package.loaded[name] = mod
    end
    original_loaded = {}
  end)

  it("loads the codediff module", function()
    reset_module_cache()
    local ok, mod = pcall(require, "codediff")
    assert.is_true(ok, "Failed to require codediff")
    assert.is_function(mod.setup)
  end)

  it("accepts an empty setup", function()
    reset_module_cache()
    assert.has_no.errors(function()
      require("codediff").setup({})
    end)
  end)

  it("accepts explorer and diff config", function()
    reset_module_cache()
    assert.has_no.errors(function()
      require("codediff").setup({
        explorer = { position = "right", width = 40 },
        diff = { algorithm = "patience" },
      })
    end)
  end)

  for _, name in ipairs({ "codediff.ui", "codediff.core.git", "codediff.core.diff" }) do
    it("loads " .. name, function()
      reset_module_cache()
      local ok, mod = pcall(require, name)
      assert.is_true(ok, "Failed to require " .. name)
      assert.is_not_nil(mod)
    end)
  end
end)
