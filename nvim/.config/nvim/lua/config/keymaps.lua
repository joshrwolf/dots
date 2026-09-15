vim.keymap.set("n", "<Esc>", "<cmd>nohlsearch<CR>", { desc = "Clear search highlighting" })
vim.keymap.set("n", "<C-d>", "<C-d>zz", { desc = "Half-page down and center" })
vim.keymap.set("n", "<C-u>", "<C-u>zz", { desc = "Half-page up and center" })

-- Herdr forwards these keys to Neovim. Move locally first, then hand focus
-- back to Herdr at a split edge. Outside Herdr, they just navigate Vim splits.
for key, direction in pairs({ h = "left", j = "down", k = "up", l = "right" }) do
  vim.keymap.set("n", "<C-" .. key .. ">", function()
    local from = vim.api.nvim_get_current_win()
    vim.cmd.wincmd(key)
    if vim.api.nvim_get_current_win() ~= from or vim.env.HERDR_ENV ~= "1" then
      return
    end
    local pane = vim.env.HERDR_PANE_ID
    if not pane or pane == "" then
      vim.notify("Cannot navigate Herdr: HERDR_PANE_ID is missing", vim.log.levels.WARN)
      return
    end
    vim.system({ "herdr", "pane", "focus", "--direction", direction, "--pane", pane }, {
      text = true,
      timeout = 2000,
    }, function(result)
      if result.code ~= 0 then
        vim.schedule(function()
          vim.notify("Herdr focus " .. direction .. " failed: " .. (result.stderr or ""), vim.log.levels.WARN)
        end)
      end
    end)
  end, { desc = "Navigate " .. direction })
end

vim.keymap.set("n", "-", "<cmd>Oil<CR>", { desc = "Open parent directory" })
vim.keymap.set({ "n", "x" }, "<leader>cf", function()
  require("conform").format({ async = true, lsp_format = "never" })
end, { desc = "Format buffer or selection" })
vim.keymap.set("n", "<leader>cd", vim.diagnostic.open_float, { desc = "Show diagnostic" })
vim.keymap.set("n", "<leader>?", function()
  require("which-key").show({ global = true })
end, { desc = "Show keymaps" })
vim.keymap.set("n", "<leader>ff", function()
  Snacks.picker.files()
end, { desc = "Find files" })
vim.keymap.set("n", "<leader>fg", function()
  Snacks.picker.grep()
end, { desc = "Search text" })
vim.keymap.set("n", "<leader>fb", function()
  Snacks.picker.buffers()
end, { desc = "Find buffers" })
vim.keymap.set("n", "<leader>fp", function()
  Snacks.picker.pickers()
end, { desc = "Find pickers" })

-- These Snacks utilities load when invoked; no separate setup is needed.
vim.keymap.set("n", "<leader>bd", function()
  Snacks.bufdelete()
end, { desc = "Delete buffer (keep splits)" })
vim.keymap.set({ "n", "x" }, "<leader>gb", function()
  Snacks.gitbrowse({ what = "file" })
end, { desc = "Browse file or selection on Git remote" })
vim.keymap.set("n", "<leader>fd", function()
  Snacks.picker.diagnostics()
end, { desc = "Find diagnostics" })
vim.keymap.set("n", "<leader>fs", function()
  Snacks.picker.lsp_symbols()
end, { desc = "Find document symbols" })
vim.keymap.set("n", "<leader>fr", function()
  Snacks.picker.recent()
end, { desc = "Find recent files" })
vim.keymap.set("n", "<leader>fn", function()
  Snacks.picker.notifications()
end, { desc = "Notification history" })
vim.keymap.set("n", "<leader>fh", function()
  Snacks.picker.git_diff()
end, { desc = "Find Git hunks" })
vim.keymap.set("n", "<leader>gd", "<cmd>CodeDiff<CR>", { desc = "Review Git changes" })
vim.keymap.set("n", "<leader>gD", function()
  require("config.branch-diff").open()
end, { desc = "Review branch changes (PR-style)" })
