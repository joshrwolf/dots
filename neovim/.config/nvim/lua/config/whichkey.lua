local M = {}

function M.setup()
  local whichkey = require "which-key"

  local dopts = { silent = true }

  local conf = {
    window = {
      border = "single",
      position = "bottom",
    },
    plugins = {
      marks = true,
      registers = true,
    },
  }

  whichkey.setup(conf)

  -- Register generic mappings
  -- whichkey.register({})

  -- Register leader based mappings
  whichkey.register({
    w = { "<cmd>w<cr>", "Save" },
    q = { "<cmd>q<cr>", "Quit" },
    e = { "<cmd>Neotree toggle<cr>", "Toggle Explorer" },
    o = { "<cmd>Neotree focus<cr>", "Focus Explorer" },

    ["<leader>"] = { "<cmd>lua require('telescope.builtin').find_files()<cr>", "Find Files" },
    ["."] = { "<cmd>lua require('telescope.builtin').buffers()<cr>", "Buffers" },

    b = {
      name = "Buffers",
      c = { "<cmd>bd!<CR>", "Close current buffer" },
    },

    f = {
      name = "Find",
      f = { "<cmd>lua require('telescope.builtin').find_files()<cr>", "Files" },
      w = { "<cmd>lua require('telescope.builtin').live_grep()<cr>", "Live Grep" },
      b = { "<cmd>lua require('telescope.builtin').buffers()<cr>", "Bufers" },
      d = { "<cmd>lua require('telescope').extensions.file_browser.file_browser()<cr>", "File Browser" },
      D = { "<cmd>lua require('telescope').extensions.file_browser.file_browser( { path = '%:p:h' } )<cr>", "File Browser" },
    },

    p = {
      name = "Packer",
      c = { "<cmd>PackerCompile<cr>", "Compile" },
      i = { "<cmd>PackerInstall<cr>", "Install" },
      s = { "<cmd>PackerSync<cr>", "Sync" },
      S = { "<cmd>PackerStatus<cr>", "Status" },
      u = { "<cmd>PackerUpdate<cr>", "Update" },
    },

    t = {
      name = "Terminal",
      f = { "<cmd>ToggleTerm direction=float<cr>", "New Float" },
      h = { "<cmd>ToggleTerm size=10 direction=horizontal<cr>", "New Horizontal" },
      v = { "<cmd>ToggleTerm size=80 direction=vertical<cr>", "New Vertical" },
    },

    j = {
      name = "Jump",
      a = { "<cmd>lua require('harpoon.mark').add_file()<cr>", "Add File" },
      j = { "<cmd>lua require('harpoon.ui').toggle_quick_menu()<cr>", "UI Menu" },
      o = { "<cmd>lua require('harpoon.cmd-ui').toggle_quick_menu()<cr>", "Command Menu" },
    },
    ["1"] = { "<cmd>lua require('harpoon.ui').nav_file(1)<cr>", "Harpoon 1" },
    ["2"] = { "<cmd>lua require('harpoon.ui').nav_file(2)<cr>", "Harpoon 2" },
    ["3"] = { "<cmd>lua require('harpoon.ui').nav_file(3)<cr>", "Harpoon 3" },
    ["9"] = { "<cmd>lua require('harpoon.term').gotoTerminal(1)<cr>", "Terminal 1" },
    ["0"] = { "<cmd>lua require('harpoon.term').gotoTerminal(2)<cr>", "Terminal 2" },

    g = {
      name = "Git",
      g = { "<cmd>lua require('utils.term').toggle_git()<cr>", "Status" },
      c = { "<cmd>lua require('config.telescope').delta_git_commits()<cr>", "Delta Commits" },
      t = { "<cmd>lua require('config.telescope').delta_git_status()<cr>", "Delta Status" },
    },
  }, { prefix = "<leader>", mode = "n", buffer = nil, silent = true, noremap = true, nowait = false })
end

return M
