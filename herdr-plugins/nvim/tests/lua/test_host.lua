local T, child = require("support").child_set()
local eq = MiniTest.expect.equality
local function setup()
  child.lua_func(function()
    jobs, completions = {}, {}
    local function execute(args, options, callback)
      local job = { args = args, options = options, waits = 0 }
      job.result = {
        code = 0,
        stdout = vim.json.encode({
          result = {
            type = "pane_current",
            pane = { pane_id = "w2:p9", tab_id = "w2:t3", workspace_id = "w2", tokens = { nvim_editor = "token" } },
          },
        }),
        stderr = "",
      }
      function job.finish(result)
        assert(not job.finished, "process callbacks run once")
        job.finished = true
        job.result = result or job.result
        if callback then
          callback(job.result)
        end
      end
      function job.wait()
        job.waits = job.waits + 1
        if not job.finished then
          job.finish()
        end
        return job.result
      end
      table.insert(jobs, job)
      return job
    end
    host = require("dots.herdr.host").new("/tmp/example-herdr.sock", execute)
    pending = host.publish("token", function(err)
      table.insert(completions, err and err.code or "ready")
    end)
  end)
end

T["publication uses live caller lookup and a bounded second command"] = function()
  setup()
  eq(
    child.lua_func(function()
      jobs[1].finish()
      assert(vim.wait(1000, function()
        return #jobs == 2
      end))
      jobs[2].finish({ code = 0, stdout = "", stderr = "" })
      assert(vim.wait(1000, function()
        return #completions == 1
      end))
      return { jobs[1].args, jobs[2].args, jobs[2].options.timeout, completions }
    end),
    {
      { "herdr", "pane", "current", "--current" },
      { "herdr", "pane", "report-metadata", "w2:p9", "--source", "dots-nvim", "--token", "nvim_editor=token" },
      1500,
      { "ready" },
    }
  )
end

T["cancelling a queued lookup cannot start metadata publication"] = function()
  setup()
  eq(
    child.lua_func(function()
      jobs[1].finish() -- callback is queued, not yet run
      pending.cancel()
      vim.wait(1000, function()
        return jobs[1].waits == 1
      end)
      return { #jobs, #completions, jobs[1].waits }
    end),
    { 1, 0, 1 }
  )
  -- Another RPC turn lets any scheduled callback run; still no publication.
  eq(child.lua_get("#jobs"), 1)
end

T["cancelling a started report settles it without a ready callback"] = function()
  setup()
  eq(
    child.lua_func(function()
      jobs[1].finish()
      assert(vim.wait(1000, function()
        return #jobs == 2
      end))
      pending.cancel()
      return jobs[2].waits
    end),
    1
  )
  eq(child.lua_get("#completions"), 0)
end

T["failed lookups and malformed results cannot publish"] = function()
  setup()
  eq(
    child.lua_func(function()
      jobs[1].finish({ code = 0, stdout = "{}", stderr = "" })
      assert(vim.wait(1000, function()
        return #completions == 1
      end))
      return { #jobs, completions }
    end),
    { 1, { "host_unavailable" } }
  )
end

T["clear reads live identity and only clears our token"] = function()
  setup()
  eq(
    child.lua_func(function()
      pending.cancel()
      host.clear("someone-else")
      local after_other = #jobs
      host.clear("token")
      return { after_other, jobs[#jobs].args }
    end),
    { 2, { "herdr", "pane", "report-metadata", "w2:p9", "--source", "dots-nvim", "--clear-token", "nvim_editor" } }
  )
end
return T
