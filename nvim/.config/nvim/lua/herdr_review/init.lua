local M = {}

M.client = require("herdr_review.client")

local configured = false

function M.open_pull_request(target)
  if vim.env.HERDR_ENV ~= "1" then
    vim.notify("review: this action needs to run inside Herdr", vim.log.levels.ERROR)
    return
  end
  if type(target) ~= "string" or target == "" then
    vim.notify("review: missing pull request URL or number", vim.log.levels.ERROR)
    return
  end
  local workspace = vim.env.HERDR_WORKSPACE_ID
  if not workspace or workspace == "" then
    vim.notify("review: the current Herdr workspace is unavailable", vim.log.levels.ERROR)
    return
  end
  local herdr = vim.env.HERDR_BIN_PATH or "herdr"
  local cwd = vim.fs.root(0, ".git") or vim.fn.getcwd()
  vim.system({
    herdr, "plugin", "pane", "open", "--plugin", "herdr-review",
    "--entrypoint", "open", "--workspace", workspace, "--cwd", cwd,
    "--env", "HERDR_REVIEW_PR=" .. target, "--focus",
  }, { text = true }, function(result)
    if result.code ~= 0 then
      vim.schedule(function()
        local detail = vim.trim(result.stderr or "")
        vim.notify(
          "review: could not hand the pull request to Herdr" .. (detail ~= "" and (": " .. detail) or ""),
          vim.log.levels.ERROR
        )
      end)
    end
  end)
end

function M.setup(opts)
  if configured then
    return
  end
  configured = true

  local codediff = require("herdr_review.codediff")
  local workflow = require("herdr_review.workflow")
  workflow.setup(opts)
  codediff.observe({
    ready = function(tabpage)
      workflow.attach(tabpage)
    end,
    updating = function(tabpage)
      workflow.detach(tabpage, false)
    end,
  })

  vim.api.nvim_create_user_command("ReviewThread", function(command)
    workflow.create_thread(command.line1, command.line2)
  end, { range = true })
  vim.api.nvim_create_user_command("ReviewRefresh", workflow.refresh, {})
  vim.api.nvim_create_user_command("ReviewResolve", workflow.resolve_at_cursor, {})
  vim.api.nvim_create_user_command("ReviewReply", workflow.reply_at_cursor, {})
  vim.api.nvim_create_user_command("ReviewStatus", workflow.status, {})
  vim.api.nvim_create_user_command("ReviewSend", workflow.request_agent, {})
  vim.api.nvim_create_user_command("ReviewRetryDispatch", workflow.retry_dispatch, {})
  vim.api.nvim_create_user_command("ReviewThreads", workflow.pick_threads, {})
  vim.api.nvim_create_user_command("ReviewOpen", workflow.open_review, {})

  local group = vim.api.nvim_create_augroup("HerdrReview", { clear = true })
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeDiffOpen",
    callback = function(event)
      local tabpage = event.data and event.data.tabpage
      workflow.attach(tabpage)
    end,
  })
  vim.api.nvim_create_autocmd({ "BufEnter", "BufWinEnter" }, {
    group = group,
    callback = function(event) workflow.attach_for_buffer(event.buf) end,
  })
  vim.api.nvim_create_autocmd("User", {
    group = group,
    pattern = "CodeDiffClose",
    callback = function(event) workflow.detach(event.data and event.data.tabpage, true) end,
  })
  vim.api.nvim_create_autocmd("FocusGained", {
    group = group,
    callback = function() workflow.attach(vim.api.nvim_get_current_tabpage(), true) end,
  })
  vim.api.nvim_create_autocmd({ "VimResized", "WinResized" }, { group = group, callback = workflow.rerender })

end

return M
