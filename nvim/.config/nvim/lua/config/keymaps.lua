-- Keymaps are automatically loaded on the VeryLazy event
-- Default keymaps that are always set: https://github.com/LazyVim/LazyVim/blob/main/lua/lazyvim/config/keymaps.lua
-- Add any additional keymaps here

vim.keymap.set({ "n" }, "<C-d>", "<C-d>zz", { remap = true })
vim.keymap.set({ "n" }, "<C-u>", "<C-u>zz", { remap = true })

-- Disable Move Lines
-- TODO: This is causing me headache
vim.keymap.del({ "n", "i", "v" }, "<A-j>")
vim.keymap.del({ "n", "i", "v" }, "<A-k>")

vim.keymap.set("n", "<c-/>", function()
  LazyVim.terminal()
end, { desc = "Open LazyVim Terminal" })

-- Tab navigation
vim.keymap.set("n", "[t", "<cmd>tabprevious<cr>", { desc = "Previous Tab" })
vim.keymap.set("n", "]t", "<cmd>tabnext<cr>", { desc = "Next Tab" })

-- herdr forwards ctrl+hjkl into this pane; at a split edge focus goes back to
-- herdr. vim-tmux-navigator is disabled under herdr and owns these keys without it.
if vim.env.HERDR_ENV == "1" then
  for key, move in pairs({
    ["<C-h>"] = { "h", "left" },
    ["<C-j>"] = { "j", "down" },
    ["<C-k>"] = { "k", "up" },
    ["<C-l>"] = { "l", "right" },
  }) do
    vim.keymap.set("n", key, function()
      local from = vim.api.nvim_get_current_win()
      vim.cmd.wincmd(move[1])
      if vim.api.nvim_get_current_win() ~= from then
        return
      end
      vim.system(
        { "herdr", "pane", "focus", "--direction", move[2], "--pane", vim.env.HERDR_PANE_ID },
        {},
        function(out)
          -- A silently dropped handoff reads as a dead key, so surface it.
          if out.code ~= 0 then
            vim.schedule(function()
              vim.notify("herdr focus " .. move[2] .. " failed: " .. (out.stderr or ""), vim.log.levels.WARN)
            end)
          end
        end
      )
    end, { desc = "Navigate " .. move[2] })
  end
end


