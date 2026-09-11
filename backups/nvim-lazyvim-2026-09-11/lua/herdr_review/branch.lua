local M = {}

-- Resolve only the selected remote's default branch; never fetch every branch
-- or infer that a fork's local main is the PR base.
function M.comparison(root, callback)
  local function git(args, next_step, optional)
    local command = { "git", "-C", root }
    vim.list_extend(command, args)
    vim.system(command, { text = true, timeout = 30000, env = { GIT_TERMINAL_PROMPT = "0" } }, function(result)
      vim.schedule(function()
        if result.code ~= 0 and not (optional and result.code == 1) then
          callback(nil, "git " .. table.concat(args, " ") .. ": " .. vim.trim(result.stderr or "command failed"))
        else
          next_step(vim.trim(result.stdout or ""))
        end
      end)
    end)
  end
  git({ "remote" }, function(output)
    local remotes = vim.split(output, "\n", { trimempty = true })
    git({ "symbolic-ref", "--quiet", "--short", "HEAD" }, function(branch)
      local function select_remote(configured)
        local remote
        for _, candidate in ipairs({ "upstream", configured, "origin" }) do
          if vim.tbl_contains(remotes, candidate) then remote = candidate; break end
        end
        remote = remote or (#remotes == 1 and remotes[1])
        if not remote then
          callback(nil, "no unambiguous base remote; configure upstream or the branch's remote")
          return
        end
        git({ "ls-remote", "--symref", remote, "HEAD" }, function(head)
          local ref = head:match("ref: (refs/heads/[^%s]+)%s+HEAD")
          if not ref then
            callback(nil, "remote " .. remote .. " does not advertise a default branch")
            return
          end
          local tracking = "refs/remotes/" .. remote .. "/" .. ref:sub(#"refs/heads/" + 1)
          git({ "fetch", "--no-tags", "--no-write-fetch-head", remote, "+" .. ref .. ":" .. tracking }, function()
            git({ "merge-base", tracking, "HEAD" }, function(base)
              callback({ base = { kind = "commit", oid = base }, target = { kind = "working_tree" } })
            end)
          end)
        end)
      end
      if branch == "" then
        select_remote("")
      else
        git({ "config", "--get", "branch." .. branch .. ".remote" }, select_remote, true)
      end
    end, true)
  end)
end

return M
