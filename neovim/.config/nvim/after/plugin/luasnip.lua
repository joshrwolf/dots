local ls = require "luasnip"

-- Keymaps
vim.keymap.set({ "i", "s" }, "<C-l>", function ()
  if ls.expand_or_jumpable() then
    ls.expand_or_jump()
  end
end, { silent = true })

vim.keymap.set({ "i", "s" }, "<C-h>", function ()
    if ls.jumpable(-1) then
      ls.jump(-1)
    end 
end, { silent = true })

-- vim.keymap.set("i", "<C-l>", function ()
--   if ls.choice_active() then
--     ls.change_choice(1)
--   end
-- end)

ls.config.set_config {
  -- remember to keep around the last snippet
  history = true,

  updateevents = "TextChanged,TextChangedI",
}

ls.snippets = {
  -- Snippets for all files
  all = {
    ls.parser.parse_snippet("expand", "-- this is what was expanded"),
  },

  -- Snippets for lua
  lua = {

  },
}

