vim.opt_local.wrap = true
vim.opt_local.linebreak = true
vim.opt_local.spell = true

-- TODO: This should better detect when we're actually in an obsidian markdown or not
vim.keymap.set("n", "gd", function ()
  if require("obsidian").util.cursor_on_markdown_link() then
    return "<cmd>ObsidianFollowLink<cr>"
  else
    return "gf"
  end
end, { noremap = false, expr = true })
