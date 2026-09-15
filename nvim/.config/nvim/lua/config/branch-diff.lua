local M = {}

local function notify(message, level, timeout)
  vim.notify("Branch diff: " .. message, level or vim.log.levels.INFO, {
    id = "branch-diff",
    timeout = timeout,
  })
end

local function fail(message)
  notify(message, vim.log.levels.ERROR, false)
end

local function failure_reason(result, timeout_seconds)
  if result.code == 124 then
    return "timed out after " .. timeout_seconds .. " seconds; try again"
  end
  local stderr = vim.trim(result.stderr or "")
  if stderr ~= "" then
    return stderr
  end
  return "process exited with code " .. tostring(result.code) .. " (signal " .. tostring(result.signal or 0) .. ")"
end

-- Local queries are bounded; network operations below run asynchronously.
local function git(cwd, args)
  local result = vim
    .system(vim.list_extend({ "git" }, args), {
      cwd = cwd,
      text = true,
      timeout = 2000,
    })
    :wait()
  if result.code ~= 0 then
    fail(failure_reason(result, 2))
    return nil
  end
  return vim.trim(result.stdout)
end

function M.open()
  local file = vim.api.nvim_buf_get_name(0)
  local cwd = vim.bo.buftype == "" and file ~= "" and vim.fs.dirname(file) or vim.fn.getcwd()
  local root = git(cwd, { "rev-parse", "--show-toplevel" })
  if not root then
    return
  end
  if vim.fn.executable("gh") ~= 1 then
    fail("GitHub CLI (gh) is required")
    return
  end
  local head = git(root, { "rev-parse", "--verify", "HEAD" })
  if not head then
    return
  end

  notify("resolving GitHub CLI's default repository…", nil, false)
  -- Let gh honor its configured default repository; remote names are not policy.
  vim.system(
    { "gh", "repo", "view", "--json", "nameWithOwner,defaultBranchRef,url,sshUrl" },
    {
      cwd = root,
      text = true,
      timeout = 30000,
      env = { GH_PROMPT_DISABLED = "1" },
    },
    vim.schedule_wrap(function(result)
      if result.code ~= 0 then
        fail("Cannot resolve GitHub repository: " .. failure_reason(result, 30))
        return
      end
      local ok, repo = pcall(vim.json.decode, result.stdout)
      local branch = ok
        and type(repo) == "table"
        and type(repo.defaultBranchRef) == "table"
        and repo.defaultBranchRef.name
      local host = ok and type(repo) == "table" and type(repo.url) == "string" and repo.url:match("^https://([^/]+)/")
      if not host or type(branch) ~= "string" or branch == "" or type(repo.nameWithOwner) ~= "string" then
        fail("GitHub repository has no usable default branch")
        return
      end
      local protocol = vim
        .system({ "gh", "config", "get", "git_protocol", "--host", host }, {
          cwd = root,
          text = true,
          timeout = 2000,
        })
        :wait()
      if protocol.code ~= 0 then
        fail("Cannot read GitHub CLI's Git protocol: " .. failure_reason(protocol, 2))
        return
      end
      local transport = vim.trim(protocol.stdout)
      if transport ~= "ssh" and transport ~= "https" then
        fail("Unsupported GitHub CLI Git protocol: " .. transport)
        return
      end
      local url = transport == "ssh" and repo.sshUrl or repo.url .. ".git"
      if type(url) ~= "string" or url == "" then
        fail("GitHub repository has no clone URL")
        return
      end
      -- Fetch by repository identity, without changing any remote or local branch.
      local base = "refs/codediff/base/" .. vim.fn.sha256(repo.url)
      notify("fetching " .. repo.nameWithOwner .. "/" .. branch .. "…", nil, false)
      local fetch = { "git" }
      if transport == "https" then
        -- Reuse gh's login without changing the user's Git credential configuration.
        vim.list_extend(fetch, {
          "-c",
          "credential.https://" .. host .. ".helper=",
          "-c",
          "credential.https://" .. host .. ".helper=!gh auth git-credential",
        })
      end
      vim.list_extend(fetch, { "fetch", "--quiet", "--no-tags", url, "+refs/heads/" .. branch .. ":" .. base })
      vim.system(
        fetch,
        {
          cwd = root,
          text = true,
          -- Large repositories can take more than 30 seconds to negotiate a fetch.
          timeout = 120000,
          env = { GIT_TERMINAL_PROMPT = "0" },
        },
        vim.schedule_wrap(function(fetched)
          if fetched.code ~= 0 then
            fail("Cannot fetch " .. repo.nameWithOwner .. "/" .. branch .. ": " .. failure_reason(fetched, 120))
            return
          end
          local revision = git(root, { "rev-parse", "--verify", base })
          if not revision then
            return
          end
          notify(repo.nameWithOwner .. "/" .. branch .. " → HEAD", nil, 3000)
          vim.api.nvim_cmd({ cmd = "CodeDiff", args = { "--repo", root, revision .. "..." .. head } }, {})
        end)
      )
    end)
  )
end

return M
