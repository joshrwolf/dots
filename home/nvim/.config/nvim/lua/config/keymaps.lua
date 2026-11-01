vim.keymap.set("n", "<Esc>", "<cmd>nohlsearch<CR>", { desc = "Clear search highlighting" })
vim.keymap.set("n", "<C-d>", "<C-d>zz", { desc = "Half-page down and center" })
vim.keymap.set("n", "<C-u>", "<C-u>zz", { desc = "Half-page up and center" })
vim.keymap.set("n", "]t", "<cmd>tabnext<CR>", { desc = "Next tab" })
vim.keymap.set("n", "[t", "<cmd>tabprevious<CR>", { desc = "Previous tab" })
vim.keymap.set("x", "<", "<gv", { desc = "Indent left and keep selection" })
vim.keymap.set("x", ">", ">gv", { desc = "Indent right and keep selection" })

-- Rex forwards these keys to Neovim. Move locally first, then hand focus back
-- to Rex at a split edge. Outside Rex, they just navigate splits.
local in_rex = vim.env.REX_BLOCK ~= nil and vim.env.REX_BLOCK ~= ""

for key, direction in pairs({ h = "left", j = "down", k = "up", l = "right" }) do
  vim.keymap.set("n", "<C-" .. key .. ">", function()
    local from = vim.api.nvim_get_current_win()
    vim.cmd.wincmd(key)
    if not in_rex or vim.api.nvim_get_current_win() ~= from then
      return
    end
    vim.system({ "rex", "focus", direction }, { text = true, timeout = 2000 }, function(result)
      if result.code ~= 0 then
        vim.schedule(function()
          vim.notify("Rex focus " .. direction .. " failed: " .. (result.stderr or ""), vim.log.levels.WARN)
        end)
      end
    end)
  end, { desc = "Navigate " .. direction })
end

vim.keymap.set("n", "-", "<cmd>Oil<CR>", { desc = "Open parent directory" })
vim.keymap.set("n", "<leader>m", function()
  require("render-markdown").toggle()
end, { desc = "Toggle Markdown rendering" })
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
vim.keymap.set({ "n", "x" }, "<leader>gY", function()
  Snacks.gitbrowse({
    what = "file",
    open = function(url)
      vim.fn.setreg("+", url)
      vim.notify("Copied Git link to clipboard")
    end,
    notify = false,
  })
end, { desc = "Copy Git link for file or selection" })
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

-- dots reviews; thread actions (]r, [r, <leader>r…) are local to review buffers.
vim.keymap.set("n", "<leader>gr", function()
  require("config.review").open()
end, { desc = "Open this worktree's latest dots review" })
vim.keymap.set("n", "<leader>fR", function()
  require("config.review").pick_reviews()
end, { desc = "Find dots reviews (all worktrees)" })
vim.keymap.set("n", "<leader>ft", function()
  require("config.review").pick_threads()
end, { desc = "Find threads in this tab's review" })
-- Lets agents open a review for the user, e.g. `nvim +Review` in a new pane.
vim.api.nvim_create_user_command("Review", function(command)
  require("config.review").open(command.args ~= "" and command.args or nil)
end, { nargs = "?", desc = "Open a dots review (default: this worktree's latest)" })
