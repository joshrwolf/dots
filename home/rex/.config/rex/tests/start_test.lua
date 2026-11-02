local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")
local jumps = require("dots.jumps")
local start = require("dots.start")

local TERMINAL = "com.superlogical.terminal"
local LEAVE = 'rex do "$HOME/.config/rex/scripts/leave.lua"'
local PI = 'pi "$@"; zsh -i; ' .. LEAVE
local SHELL_ONLY = "zsh -i; " .. LEAVE
local DOTS = "/home/me/src/dots"
local API = "/home/me/src/api"
local ZOXIDE = "zoxide query --list"
local HOME = "/home/me"
local RULES = HOME .. "/.config/rex/checkouts"
local SET_DEFAULT = 'gh repo set-default "$1" && shift && exec "$@"'
local CLONE =
  'gh repo clone "$1" "$2" && cd "$2" && gh repo set-default "$1" && shift 2 && exec "$@"'
-- Where the rules below place example/web and work-org/api.
local WEB = HOME .. "/dev/personal/example/web"
local WORK_API = HOME .. "/dev/work/api"

---@type dots.Repo[]
local REPOS = {
  { path = DOTS, remotes = { "example/dots" } },
  { path = API, remotes = { "example/api", "fork/api" } },
  { path = "/home/me/src/café-notes", remotes = {} },
}

---`argv` run by `script` with `args` before it, as a PR start wraps worktrunk.
---@param script string
---@param args string[]
---@param argv string[]
---@return string[]
local function wrapped(script, args, argv)
  local out = { "zsh", "-c", script, "zsh" }
  for _, list in ipairs({ args, argv }) do
    for _, arg in ipairs(list) do
      out[#out + 1] = arg
    end
  end
  return out
end

---@param dir string
---@return string
local function git_dir(dir)
  return "git -C " .. dir .. " rev-parse --path-format=absolute --git-common-dir"
end

---@param repo string
---@param branch string
---@return string
local function show_ref(repo, branch)
  return "git -C " .. repo .. " show-ref --verify --quiet refs/heads/" .. branch
end

---@param repo string
---@return string
local function for_each_ref(repo)
  return "git -C " .. repo .. " for-each-ref --format=%(refname) refs/heads/"
end

---@param repo string
---@return string
local function worktrees(repo)
  return "git -C " .. repo .. " worktree list --porcelain -z"
end

---`git for-each-ref` output for branches.
---@param names string[]
---@return table
local function refs(names)
  local lines = {}
  for _, name in ipairs(names) do
    lines[#lines + 1] = "refs/heads/" .. name .. "\n"
  end
  return { stdout = table.concat(lines) }
end

---`git worktree list --porcelain -z` output for `{ path, branch }` pairs.
---@param list string[][]
---@return table
local function porcelain(list)
  local out = {}
  for _, entry in ipairs(list) do
    out[#out + 1] = "worktree "
      .. entry[1]
      .. "\0HEAD abc\0branch refs/heads/"
      .. entry[2]
      .. "\0\0"
  end
  return { stdout = table.concat(out) }
end

---The command a plan runs: `wt switch` with `args`, then Pi with `prompt`,
---or only a shell when `script` says so.
---@param args string[]
---@param prompt? string
---@param script? string
---@return string[]
local function switch(args, prompt, script)
  local argv = { "wt", "switch", "--no-cd" }
  for _, arg in ipairs(args) do
    argv[#argv + 1] = arg
  end
  for _, arg in ipairs({ "-x", "zsh", "--", "-ic", script or PI, "zsh" }) do
    argv[#argv + 1] = arg
  end
  argv[#argv + 1] = prompt
  return argv
end

---The `.git` files of a checkout whose origin is GitHub's `slug`.
---@param dir string
---@param slug string
---@return table<string, string>
local function checkout(dir, slug)
  return {
    [dir .. "/.git/HEAD"] = "ref: refs/heads/main\n",
    [dir .. "/.git/config"] = '[remote "origin"]\n\turl = https://github.com/' .. slug .. "\n",
  }
end

---Stand in for the filesystem: DOTS and API are checkouts with GitHub
---remotes, and `more` adds files; anything else is not a repo. The rule
---directory has the default rule and one for work-org.
---@param t tests.T
---@param more? table<string, string>
local function checkouts_on_disk(t, more)
  local files = {
    [RULES .. "/default.json"] = '{ "version": 1, "rules": { "*": "~/dev/personal/{owner}/{name}" } }',
    [RULES .. "/work.json"] = '{ "version": 1, "rules": { "work-org/*": "~/dev/work/{name}" } }',
    [DOTS .. "/.git/HEAD"] = "ref: refs/heads/main\n",
    [DOTS .. "/.git/config"] = '[remote "origin"]\n\turl = git@github.com:example/dots.git\n',
    [API .. "/.git/HEAD"] = "ref: refs/heads/main\n",
    [API .. "/.git/config"] = '[remote "origin"]\n\turl = https://github.com/example/api\n',
  }
  for path, text in pairs(more or {}) do
    files[path] = text
  end
  t.stub(start, "read", function(path)
    return files[path]
  end)
end

-- The user's session, with the picker open over a shell in the dots
-- checkout, and a session at an API worktree.
---@return tests.FakeRex
local function rex_with_picker()
  local function session(id, window, blocks)
    local placed = {}
    for _, block in ipairs(blocks) do
      placed[#placed + 1] = { block_id = block, creator_name = TERMINAL }
    end
    return {
      session_id = id,
      label = id,
      view = {
        session_id = id,
        revision = 1,
        active_window_id = window,
        windows = { { window_id = window, layers = { { blocks = placed } } } },
      },
    }
  end
  return {
    sessions = {
      session("s1", "w1", { "shell", "picker" }),
      session("s2", "w2", { "pi" }),
      session("created", "created-window", {}),
    },
    clients = { { client_id = "mine", info = { kind = "app" }, session_ids = { "s1" } } },
    commands = {
      [git_dir(DOTS .. "/docs")] = { stdout = DOTS .. "/.git\n" },
      [git_dir("/tmp")] = { ok = false },
      [ZOXIDE] = { stdout = API .. "\n" .. DOTS .. "/docs\n" .. DOTS .. "\n" },
      ["ls -1 " .. RULES] = { stdout = "default.json\nwork.json\n" },
    },
  }
end

local picker = { session_id = "s1", block_id = "picker" }

---What Go collected, without the picker.
---@type dots.Snapshot
local snap = {
  sessions = {
    {
      id = "s1",
      label = "dots",
      blocks = { { id = "shell", window_id = "w1", tab = 1, cwd = DOTS } },
    },
    {
      id = "s2",
      label = "api:ABC-12",
      blocks = { { id = "pi", window_id = "w2", tab = 1, cwd = "/home/me/.wt/api/abc-12" } },
    },
  },
  worktrees = {},
  dirs = {},
}

---@param session_id string
---@return tests.Call
local function shows(session_id)
  return {
    method = "client.perform",
    args = { client_id = "mine", action = "session.select", args = { session_id = session_id } },
  }
end

return {
  ["argv keeps the script constant and the prompt one argument"] = function()
    for _, prompt in ipairs({
      'it\'s "quoted"',
      "two\nlines",
      "$HOME and ${PATH}",
      "`date` and $(date)",
      "; rm -rf / #",
      "",
    }) do
      expect.equal(start.argv("abc-1", true, "pi", prompt), switch({ "--create", "abc-1" }, prompt))
    end
    expect.equal(start.argv("pr:12", false, "pi"), switch({ "pr:12" }))
    -- A new session opens at a shell, without Pi.
    expect.equal(
      start.argv("fresh", true, "shell"),
      switch({ "--create", "fresh" }, nil, SHELL_ONLY)
    )
  end,

  ["PR URLs name a repo and a number, with any page or query"] = function()
    local want = {
      slug = "example/api",
      repo = "api",
      number = "42",
      url = "https://github.com/example/api/pull/42",
    }
    for _, url in ipairs({
      "https://github.com/example/api/pull/42",
      " https://github.com/example/api/pull/42/ ",
      "https://github.com/example/api/pull/42/files",
      "https://github.com/example/api/pull/42/commits/abc123",
      "https://github.com/example/api/pull/42?w=1",
      "https://github.com/example/api/pull/42#discussion_r1",
      "https://github.com/example/api/pull/42/files?w=1#diff-1",
    }) do
      expect.equal({ url, start.pr(url) }, { url, want })
    end
    expect.equal(start.pr("https://github.com/my-org/my_repo.js/pull/7").slug, "my-org/my_repo.js")
  end,

  ["anything else is not a PR URL, and the error lists the valid form"] = function()
    for _, text in ipairs({
      "https://github.com/example/api/issues/42",
      "https://github.com/example/api/pull/",
      "https://github.com/example/api/pull/0",
      "https://github.com/example/api/pull/42x",
      "https://github.com/example/api/pull/42 review it",
      "http://github.com/example/api/pull/42",
      "https://gitlab.com/example/api/pull/42",
      "pr:42",
      "42",
    }) do
      local pr, err = start.pr(text)
      expect.equal({ text, pr, err }, {
        text,
        nil,
        text .. " is not a GitHub PR URL; expected one like https://github.com/owner/repo/pull/123",
      })
    end
    expect.equal(
      { start.pr(" ") },
      { nil, "paste a GitHub PR URL, like https://github.com/owner/repo/pull/123" }
    )
  end,

  ["Linear issues are IDs in any case or issue URLs"] = function()
    expect.equal(
      start.issue("abc-123"),
      { id = "ABC-123", team = "abc", branch = "abc-123", prompt = "ABC-123" }
    )
    expect.equal(
      start.issue(" Ab2-7 "),
      { id = "AB2-7", team = "ab2", branch = "ab2-7", prompt = "AB2-7" }
    )
    local url = {
      id = "ABC-123",
      team = "abc",
      branch = "abc-123",
      prompt = "https://linear.app/team/issue/ABC-123",
    }
    for _, text in ipairs({
      "https://linear.app/team/issue/ABC-123",
      "https://linear.app/team/issue/ABC-123/fix-the-login-bug",
      "https://linear.app/team/issue/abc-123/fix?view=1",
      "https://linear.app/team/issue/ABC-123#comment-1",
    }) do
      expect.equal({ text, start.issue(text) }, { text, url })
    end
  end,

  ["anything else is not a Linear issue, and the error lists the valid forms"] = function()
    for _, text in ipairs({
      "ABC",
      "ABC-",
      "ABC-0",
      "ABC-012",
      "123-45",
      "ABC-123 fix it",
      "abc_123",
      "https://linear.app/team/project/ABC-123",
      "https://linear.app/team/issue/ABC-123x",
      "https://example.com/team/issue/ABC-123",
    }) do
      local issue, err = start.issue(text)
      expect.equal({ text, issue, err }, {
        text,
        nil,
        text
          .. " is not a Linear issue; expected an ID or URL like ABC-123 or https://linear.app/team/issue/ABC-123",
      })
    end
  end,

  ["GitHub remotes in any URL form; other hosts are not"] = function()
    for _, url in ipairs({
      "git@github.com:example/tool.git",
      "ssh://git@github.com/example/tool.git",
      "https://github.com/example/tool",
      "https://github.com/example/tool.git/",
      "git://github.com/example/tool.git",
    }) do
      expect.equal({ url, start.github_remote(url) }, { url, "example/tool" })
    end
    expect.equal(start.github_remote("https://other.example/example/tool.git"), nil)
    expect.equal(start.github_remote("https://github.com/example"), nil)
    expect.equal(start.github_remote("https://github.com/example/tool/extra"), nil)
  end,

  ["a git config yields the GitHub URL of each remote, in order"] = function()
    local config = table.concat({
      "[core]",
      "\tbare = false",
      "\turl = https://github.com/not/a-remote",
      '[remote "origin"]',
      "\turl = git@github.com:example/tool.git",
      "\tfetch = +refs/heads/*:refs/remotes/origin/*",
      '[remote "upstream"]',
      '\turl = "https://github.com/upstream/tool"',
      '[remote "mirror"]',
      "\turl = https://other.example/example/tool.git",
      '[branch "main"]',
      "\tremote = origin",
    }, "\n")
    expect.equal(start.config_remotes(config), { "example/tool", "upstream/tool" })
  end,

  ["gh's default repo is the remote it marked, or the repo it names"] = function()
    -- As `gh repo clone` leaves a fork: its parent is the default.
    local fork = table.concat({
      '[remote "origin"]',
      "\turl = git@github.com:me/project.git",
      '[remote "upstream"]',
      "\turl = https://github.com/parent/project.git",
      "\tgh-resolved = base",
      '[branch "main"]',
      "\tremote = origin",
    }, "\n")
    expect.equal(start.config_default(fork), "parent/project")
    -- The mark may come before the URL, and on the last section.
    expect.equal(
      start.config_default(
        '[remote "origin"]\n\tgh-resolved = base\n\turl = https://github.com/me/project\n'
      ),
      "me/project"
    )
    -- A default that is no remote's repo is named.
    expect.equal(
      start.config_default(
        '[remote "origin"]\n\turl = https://github.com/me/project\n\tgh-resolved = other/project\n'
      ),
      "other/project"
    )
    expect.equal(
      start.config_default('[remote "origin"]\n\turl = https://github.com/me/project\n'),
      nil
    )
    expect.equal(
      start.config_default(
        '[remote "origin"]\n\turl = https://other.example/me/project\n\tgh-resolved = base\n'
      ),
      nil
    )
  end,

  ["a PR in a fork's checkout makes its repo gh's default unless it is already"] = function(t)
    local fork = HOME .. "/dev/personal/me/project"
    checkouts_on_disk(t, {
      [fork .. "/.git/HEAD"] = "ref: refs/heads/main\n",
      [fork .. "/.git/config"] = table.concat({
        '[remote "origin"]',
        "\turl = https://github.com/me/project.git",
        '[remote "upstream"]',
        "\turl = https://github.com/parent/project.git",
        "\tgh-resolved = base",
      }, "\n"),
    })
    local fake = rex_with_picker()
    fake.commands[ZOXIDE] = { stdout = fork .. "\n" }
    fake_rex.install(t, fake)
    local url = "https://github.com/me/project/pull/4"
    local review = switch({ "pr:4" }, "/skill:review " .. url)
    expect.equal(
      assert(start.plan("pr", url, "/tmp", HOME)).argv,
      wrapped(SET_DEFAULT, { "me/project" }, review)
    )
    url = "https://github.com/parent/project/pull/4"
    expect.equal(
      assert(start.plan("pr", url, "/tmp", HOME)).argv,
      switch({ "pr:4" }, "/skill:review " .. url)
    )
  end,

  ["known repos are the directories with a .git directory, once each"] = function()
    local files = {
      ["/src/a/.git/HEAD"] = "ref: refs/heads/main\n",
      ["/src/a/.git/config"] = '[remote "origin"]\n\turl = git@github.com:example/a.git\n',
      ["/src/b/.git/HEAD"] = "ref: refs/heads/main\n",
    }
    local known = start.known({ "/src/a", "/src/worktree", "/src/b", "/src/a" }, function(path)
      return files[path]
    end)
    expect.equal(known, {
      { path = "/src/a", remotes = { "example/a" } },
      { path = "/src/b", remotes = {} },
    })
  end,

  ["a PR goes to the one repo with its remote, whatever the case"] = function()
    expect.equal(
      start.plan_pr(assert(start.pr("https://github.com/Fork/API/pull/9/files")), REPOS, nil, HOME),
      {
        repo = API,
        label = "API#9",
        -- Without a gh default, worktrunk would pick one of its two remotes.
        argv = wrapped(
          SET_DEFAULT,
          { "Fork/API" },
          switch({ "pr:9" }, "/skill:review https://github.com/Fork/API/pull/9")
        ),
        pr = "https://github.com/Fork/API/pull/9",
      }
    )
  end,

  ["a PR without one matching repo lists the candidates"] = function()
    expect.equal(
      { start.plan_pr(assert(start.pr("https://github.com/example/web/pull/1")), REPOS, nil, HOME) },
      {
        nil,
        "no known repo has the GitHub remote example/web, and no rule in ~/.config/rex/checkouts "
          .. "places it; known: dots (example/dots), api (example/api), café-notes (no GitHub remote)",
      }
    )
    local twice = { REPOS[1], { path = "/home/me/other/dots", remotes = { "example/dots" } } }
    expect.equal({
      start.plan_pr(assert(start.pr("https://github.com/example/dots/pull/1")), twice, nil, HOME),
    }, {
      nil,
      "example/dots is the remote of several repos: dots (example/dots), dots (example/dots)",
    })
  end,

  ["a branch is for an issue or a team only as a whole word"] = function()
    local issue = assert(start.issue("ABC-12"))
    for branch, want in pairs({
      ["abc-12"] = true,
      ["fix/ABC-12-login"] = true,
      ["me/abc-12"] = true,
      ["abc-123"] = false,
      ["xabc-12"] = false,
      ["abc-1"] = false,
    }) do
      expect.equal({ branch, start.for_issue(branch, issue) }, { branch, want })
    end
    for branch, want in pairs({
      ["abc-1"] = true,
      ["fix/abc-99"] = true,
      ["abc-x"] = false,
      ["xabc-1"] = false,
      ["abcd-1"] = false,
    }) do
      expect.equal({ branch, start.for_team(branch, "abc") }, { branch, want })
    end
  end,

  ["an issue goes to the repo with its branch, then the one with most team branches"] = function()
    local issue = assert(start.issue("abc-12"))
    local branches = {
      ["/r/one"] = { "main", "abc-1", "abc-2", "abc-3" },
      ["/r/two"] = { "main", "fix/abc-12", "abc-4" },
      ["/r/three"] = { "main", "xyz-1" },
    }
    local function choose(chosen)
      return start.choose(chosen, { "/r/one", "/r/two", "/r/three" }, function(repo)
        return branches[repo] or {}
      end)
    end
    expect.equal(choose(issue), "/r/two")
    branches["/r/one"][#branches["/r/one"] + 1] = "abc-12"
    expect.equal(
      { "two have it: most team", choose(issue) },
      { "two have it: most team", "/r/one" }
    )
    local other = assert(start.issue("abc-99"))
    expect.equal({ "most team branches", choose(other) }, { "most team branches", "/r/one" })
    branches["/r/two"] = { "abc-5", "abc-6", "abc-7", "abc-8" }
    expect.equal({ "a tie", choose(other) }, { "a tie", nil })
    expect.equal({ "no team", choose(assert(start.issue("new-1"))) }, { "no team", nil })
  end,

  ["an issue creates its branch in the chosen repo, or switches to it"] = function(t)
    checkouts_on_disk(t)
    local fake = rex_with_picker()
    fake.commands[for_each_ref(DOTS)] = refs({ "main", "abc-1" })
    fake.commands[for_each_ref(API)] = refs({ "main", "me/abc-12-login" })
    fake.commands[show_ref(API, "abc-12")] = { ok = false }
    fake_rex.install(t, fake)
    expect.equal(start.plan("issue", " abc-12 ", DOTS .. "/docs"), {
      repo = API,
      label = "api:ABC-12",
      argv = switch({ "--create", "abc-12" }, "ABC-12"),
      branch = "abc-12",
    })
    fake.commands[show_ref(API, "abc-12")] = {}
    expect.equal(
      assert(start.plan("issue", "https://linear.app/team/issue/ABC-12/title")).argv,
      switch({ "abc-12" }, "https://linear.app/team/issue/ABC-12")
    )
  end,

  ["an issue no repo has branches for lists the known repos"] = function(t)
    checkouts_on_disk(t)
    local fake = rex_with_picker()
    fake.commands[for_each_ref(DOTS)] = refs({ "main" })
    fake.commands[for_each_ref(API)] = refs({ "main" })
    fake_rex.install(t, fake)
    expect.equal({ start.plan("issue", "ABC-12", DOTS .. "/docs") }, {
      nil,
      "no repo has a branch for abc-12 or most branches for team abc; "
        .. "known: dots (example/dots), api (example/api)",
    })
  end,

  ["a generated name is two words, drawn again while a branch has it"] = function()
    local draws = { 1, 1, 46, 34 }
    local function random(n)
      local value = table.remove(draws, 1)
      assert(value <= n, "draw out of range")
      return value
    end
    expect.equal(start.name({ "main", "amber-aspen" }, random), "twilight-prairie")
    expect.equal(draws, {})
  end,

  ["a new session starts a generated worktree in the selected row's repo"] = function(t)
    local fake = rex_with_picker()
    fake.commands[for_each_ref(DOTS)] = refs({ "main" })
    fake_rex.install(t, fake)
    local draws = { 46, 34 }
    expect.equal(
      start.plan_new(DOTS .. "/docs", function()
        return table.remove(draws, 1)
      end),
      {
        repo = DOTS,
        label = "dots:twilight-prairie",
        argv = switch({ "--create", "twilight-prairie" }, nil, SHELL_ONLY),
        branch = "twilight-prairie",
      }
    )
  end,

  ["a new session without a row in a repo asks for one"] = function(t)
    fake_rex.install(t, rex_with_picker())
    for _, dir in ipairs({ "/tmp", false }) do
      expect.equal(
        { start.plan_new(dir or nil, math.random) },
        { nil, "select a repo first: type / and pick one" }
      )
    end
  end,

  ["a malformed line fails before reading git"] = function(t)
    local fake = fake_rex.install(t, rex_with_picker())
    for _, case in ipairs({
      {
        "issue",
        "abc 12",
        "abc 12 is not a Linear issue; expected an ID or URL like ABC-123",
      },
      { "pr", "pr:3", "pr:3 is not a GitHub PR URL; expected one like" },
    }) do
      local plan, err = start.plan(case[1], case[2], DOTS .. "/docs")
      expect.equal({ case[2], plan, err and err:sub(1, #case[3]) }, { case[2], nil, case[3] })
    end
    expect.equal(fake.only("exec"), {})
  end,

  ["a PR's repo may be the terminal's even if zoxide does not know it"] = function(t)
    checkouts_on_disk(t)
    local fake = rex_with_picker()
    fake.commands[ZOXIDE] = { stdout = "" }
    fake_rex.install(t, fake)
    expect.equal(
      assert(start.plan("pr", "https://github.com/example/dots/pull/3", DOTS .. "/docs", HOME)).repo,
      DOTS
    )
  end,

  ["a PR's checkout at its rule path is found though zoxide does not list it"] = function(t)
    checkouts_on_disk(t, checkout(WORK_API, "work-org/api"))
    local fake = rex_with_picker()
    fake.commands[ZOXIDE] = { stdout = "" }
    fake_rex.install(t, fake)
    local url = "https://github.com/work-org/api/pull/5"
    expect.equal(start.plan("pr", url, "/tmp", HOME), {
      repo = WORK_API,
      label = "api#5",
      argv = switch({ "pr:5" }, "/skill:review " .. url),
      pr = url,
    })
  end,

  ["a checkout with the PR's remote elsewhere is used, not cloned beside"] = function(t)
    checkouts_on_disk(t)
    local fake = fake_rex.install(t, rex_with_picker())
    expect.equal(
      assert(start.plan("pr", "https://github.com/example/api/pull/5", "/tmp", HOME)).repo,
      API
    )
    -- Nothing asks whether its rule path is free to clone into.
    for _, call in ipairs(fake.only("exec")) do
      expect.equal({ call.args[1], call.args[2] == "-A" }, { call.args[1], false })
    end
  end,

  ["of several checkouts with the PR's remote, the one at its rule path wins"] = function(t)
    local other = "/home/me/old/web"
    local files = checkout(WEB, "example/web")
    for path, text in pairs(checkout(other, "example/web")) do
      files[path] = text
    end
    checkouts_on_disk(t, files)
    local fake = rex_with_picker()
    fake.commands[ZOXIDE] = { stdout = other .. "\n" }
    fake_rex.install(t, fake)
    expect.equal(
      assert(start.plan("pr", "https://github.com/example/web/pull/1", "/tmp", HOME)).repo,
      WEB
    )
  end,

  ["a PR without a checkout clones its repo at the rule path, then reviews it"] = function(t)
    checkouts_on_disk(t)
    local url = "https://github.com/example/web/pull/1"
    local review = switch({ "pr:1" }, "/skill:review " .. url)
    -- The clone makes example/web gh's default even when it is a fork.
    local clone = wrapped(CLONE, { "example/web", WEB }, review)
    -- Absent, or an empty directory.
    for _, listed in ipairs({ { ok = false }, { stdout = "" } }) do
      local fake = rex_with_picker()
      fake.commands["ls -A " .. WEB] = listed
      fake_rex.install(t, fake)
      expect.equal(start.plan("pr", url, "/tmp", HOME), {
        repo = WEB,
        label = "web#1",
        argv = clone,
        pr = url,
        clone = "example/web",
        cwd = HOME,
      })
    end
  end,

  ["a PR's rule path that holds something else is not cloned into"] = function(t)
    checkouts_on_disk(t)
    local fake = rex_with_picker()
    fake.commands["ls -A " .. WEB] = { stdout = "notes.txt\n" }
    fake_rex.install(t, fake)
    expect.equal({ start.plan("pr", "https://github.com/example/web/pull/1", "/tmp", HOME) }, {
      nil,
      "no known repo has the GitHub remote example/web; not cloning into its checkout path "
        .. "~/dev/personal/example/web (rule * in ~/.config/rex/checkouts/default.json), "
        .. "which is not empty and not a checkout; known: api (example/api), dots (example/dots)",
    })
  end,

  ["a PR's rule path that is another repo's checkout is not used"] = function(t)
    checkouts_on_disk(t, checkout(WORK_API, "someone/api"))
    fake_rex.install(t, rex_with_picker())
    expect.equal({ start.plan("pr", "https://github.com/work-org/api/pull/5", "/tmp", HOME) }, {
      nil,
      "no known repo has the GitHub remote work-org/api; its checkout path "
        .. "~/dev/work/api (rule work-org/* in ~/.config/rex/checkouts/work.json) "
        .. "is a checkout of someone/api; known: api (someone/api), api (example/api), dots (example/dots)",
    })
  end,

  ["a broken rule file stops a PR start"] = function(t)
    checkouts_on_disk(
      t,
      { [RULES .. "/work.json"] = '{ "version": 1, "rules": { "work-org/*": "dev/{name}" } }' }
    )
    fake_rex.install(t, rex_with_picker())
    expect.equal(
      { start.plan("pr", "https://github.com/example/dots/pull/3", DOTS .. "/docs", HOME) },
      {
        nil,
        RULES
          .. "/work.json: rule work-org/* must be a path starting with ~/ or / that uses only {owner} and {name}",
      }
    )
  end,

  ["a start that clones runs from home and skips looking for a worktree"] = function(t)
    t.stub(jumps, "recent", {})
    local installed = fake_rex.install(t, rex_with_picker())
    local argv = { "zsh", "-c", CLONE, "zsh", "example/web", WEB, "wt" }
    start.open(picker, {
      repo = WEB,
      label = "web#1",
      argv = argv,
      pr = "https://github.com/example/web/pull/1",
      clone = "example/web",
      cwd = HOME,
    }, snap)
    expect.equal(installed.only("exec"), {})
    expect.equal(
      installed.only("session.create")[1].args.initial_windows[1].layout,
      rex.layout.block({
        flavor = "com.superlogical.terminal.shell",
        options = { command = argv, cwd = HOME, exit = { quick_exit_threshold_ms = 300000 } },
      })
    )
  end,

  ["the worktree of a branch comes from git's porcelain listing"] = function()
    local listing = porcelain({ { DOTS, "main" }, { "/wt/dots/fix", "fix" } }).stdout
      .. "worktree /wt/detached\0HEAD abc\0detached\0\0"
    expect.equal(start.worktree_in(listing, "fix"), "/wt/dots/fix")
    expect.equal(start.worktree_in(listing, "main"), DOTS)
    expect.equal(start.worktree_in(listing, "fi"), nil)
  end,

  ["a start with a session at its worktree goes there and types nothing"] = function(t)
    t.stub(jumps, "recent", {})
    local fake = rex_with_picker()
    fake.commands[worktrees(API)] =
      porcelain({ { API, "main" }, { "/home/me/.wt/api/abc-12", "abc-12" } })
    local installed = fake_rex.install(t, fake)
    start.open(picker, {
      repo = API,
      label = "api:ABC-12",
      argv = switch({ "abc-12" }, "ABC-12"),
      branch = "abc-12",
    }, snap)
    expect.equal(installed.only("session.create", "session.focus_block", "client.perform"), {
      { method = "session.focus_block", args = { session_id = "s2", block_id = "pi" } },
      shows("s2"),
    })
    expect.equal(jumps.recent, {
      { session_id = "s2", window_id = "w2" },
      { session_id = "s1", window_id = "w1" },
    })
  end,

  ["a PR's worktree is found by its head branch"] = function(t)
    local fake = rex_with_picker()
    local url = "https://github.com/example/api/pull/3"
    fake.commands["gh pr view " .. url .. " --json headRefName --jq .headRefName"] =
      { stdout = "abc-12\n" }
    fake.commands[worktrees(API)] = porcelain({ { "/home/me/.wt/api/abc-12", "abc-12" } })
    local installed = fake_rex.install(t, fake)
    start.open(picker, { repo = API, label = "api#3", argv = switch({ "pr:3" }), pr = url }, snap)
    expect.equal(installed.only("session.create", "client.perform"), { shows("s2") })
  end,

  ["otherwise a start creates a session running the plan, shows it, and records the jump"] = function(
    t
  )
    t.stub(jumps, "recent", {})
    local fake = rex_with_picker()
    local url = "https://github.com/example/api/pull/3"
    fake.commands["gh pr view " .. url .. " --json headRefName --jq .headRefName"] = { ok = false }
    local installed = fake_rex.install(t, fake)
    local argv = switch({ "pr:3" }, "/skill:review " .. url)
    start.open(picker, { repo = API, label = "api#3", argv = argv, pr = url }, snap)
    expect.equal(installed.only("session.create", "client.perform"), {
      {
        method = "session.create",
        args = {
          label = "api#3",
          initial_windows = {
            {
              layout = rex.layout.block({
                flavor = "com.superlogical.terminal.shell",
                options = { command = argv, cwd = API, exit = { quick_exit_threshold_ms = 300000 } },
              }),
            },
          },
        },
      },
      shows("created"),
    })
    expect.equal(jumps.recent, {
      { session_id = "created", window_id = "created-window" },
      { session_id = "s1", window_id = "w1" },
    })
  end,
}
