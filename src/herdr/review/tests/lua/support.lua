local M = {}

function M.child_set()
  local child = MiniTest.new_child_neovim()
  local set = MiniTest.new_set({
    hooks = {
      pre_case = function()
        child.restart({ "-u", vim.env.HERDR_REVIEW_TESTS .. "/lua/child.lua", "-i", "NONE" })
      end,
      post_case = child.stop,
      post_once = child.stop,
    },
  })
  return set, child
end

return M
