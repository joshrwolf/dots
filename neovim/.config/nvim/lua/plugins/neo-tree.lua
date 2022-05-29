local M = {}

-- Remove any deprecated v1 features
vim.cmd([[ let g:neo_tree_remove_legacy_commands = 1 ]])

function M.config()
    require('neo-tree').setup({
        close_if_last_window = true,
        popup_border_style = 'rounded',
        window = {
          position = "left",
          width = 40,
          mapping_options = {
            noremap = true,
            nowait = true,
          },
          mappings = {
            -- Non-default mappings
            ["<tab>"] = function (state)
              local node = state.tree:get_node()
              if require("neo-tree.utils").is_expandable(node) then
                state.commands["toggle_node"](state)
              else
                state.commands["open"](state)
                vim.cmd("Neotree reveal")
              end
            end
          }
        },
        filesystem = {
          filtered_items = {
            hide_dotfiles = false,
            hide_gitignored = false,
            never_show = {
              ".DS_Store",
              "thumbs.db",
            },
          },
        },
    })
end

return M
