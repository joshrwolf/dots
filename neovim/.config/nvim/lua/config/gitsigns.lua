local M = {}

function M.setup()
  require("gitsigns").setup {
    signs = {
      add = { text = "▎" },
      change = { text = "▎" },
      delete = { text = "▎" },
      topdelete = { text = "契" },
      changedelete = { text = "▎" },
    },
    keymaps = {
      noremap = true,
      buffer = true,
      ["n ]c"] = { expr = true, "&diff ? ']c' : '<cmd>lua require\"gitsigns\".next_hunk()<CR>'" },
      ["n [c"] = { expr = true, "&diff ? '[c' : '<cmd>lua require\"gitsigns\".prev_hunk()<CR>'" },
      ["o ih"] = ':<C-U>lua require("gitsigns").select_hunk()<cr>',
      ["x ih"] = ':<C-U>lua require("gitsigns").select_hunk()<cr>',
    },
    on_attach = function(bufnr)
      local opts = { buffer = bufnr, noremap = true, silent = true }
      local maps = {
        g = {
          name = "Git",
          j = { "<cmd>lua require('gitsigns').next_hunk()<cr>", "Next Hunk" },
          k = { "<cmd>lua require('gitsigns').prev_hunk()<cr>", "Prev Hunk" },
          p = { "<cmd>lua require('gitsigns').preview_hunk()<cr>", "Preview Hunk" },
          -- l = { "<cmd>lua require('gitsigns').blame_line()<cr>", "Blame" },
          s = { "<cmd>lua require('gitsigns').stage_hunk()<cr>", "Stage Hunk" },
          u = { "<cmd>lua require('gitsigns').undo_stage_hunk()<cr>", "Undo Stage Hunk" },
          r = { "<cmd>lua require('gitsigns').reset_hunk()<cr>", "Reset Hunk" },
        }
      }

      require("which-key").register(maps, { prefix = "<leader>", opts })
    end,
    preview_config = {
      border = "single",
      style = "minimal",
      relative = "cursor",
      row = 0,
      col = 1,
    },
  }
end

return M
