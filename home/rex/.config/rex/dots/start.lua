-- Start: work in a worktree with Pi, from a PR URL, a Linear issue, or a
-- repo for a new session. Input is parsed deterministically and a wrong shape
-- is an error that lists the valid forms. The repo is chosen from local git
-- alone; a session already at the worktree is shown instead of a second one.
-- A PR whose repo has no checkout is cloned where `dots.checkouts` places it.
-- A new session's one terminal runs worktrunk, which creates or reuses the
-- worktree (hooks and approvals included) and starts Pi there; when Pi exits,
-- the terminal becomes a shell in the worktree, and when worktrunk fails, its
-- error stays on screen.
local app = require("dots.app")
local checkouts = require("dots.checkouts")
local jumps = require("dots.jumps")
local must = require("dots.result").must
local process = require("dots.process")
local sessions = require("dots.sessions")
local snapshot = require("dots.snapshot")

local SHELL = "com.superlogical.terminal.shell"
-- zoxide lists directories most frecent first; this many are considered.
local DIR_LIMIT = 200
-- What a start's terminal runs in the worktree, ending in an interactive
-- shell; leaving that shell ends the session when its tab is the last one
-- (`scripts/leave.lua`), which Rex would otherwise keep with no windows.
-- Each is constant: user text reaches Pi as an argument and is never shell
-- code. A new session opens at the shell, for the user to start what it
-- needs; PR and issue starts run Pi with their prompt first.
local LEAVE = 'rex do "$HOME/.config/rex/scripts/leave.lua"'
local SCRIPTS = {
  pi = 'pi "$@"; zsh -i; ' .. LEAVE,
  shell = "zsh -i; " .. LEAVE,
}
-- A start that fails within this long (worktrunk errors, refused hook
-- approvals, a missing PR) keeps its terminal open to show why; any other
-- exit, such as leaving the shell after Pi, closes it and with it the session.
local FAILURE_WINDOW_MS = 300000
-- Clones `$1` into `$2`, then runs the rest there. gh uses its own
-- authentication and the user's preferred Git protocol on every host.
local CLONE = 'gh repo clone "$1" "$2" && cd "$2" && shift 2 && exec "$@"'

-- The valid forms of each start's line. Errors name them briefly, because the
-- picker shows them in a few lines above its list.
local PR_FORMS = "https://github.com/owner/repo/pull/123"
local ISSUE_FORMS = "ABC-123 or https://linear.app/team/issue/ABC-123"

-- A new session's home worktree is named by one of each, like
-- `twilight-prairie`; its branch gets a real name when a PR goes up.
local ADJECTIVES = {
  "amber",
  "ancient",
  "autumn",
  "bold",
  "brave",
  "bright",
  "calm",
  "clever",
  "cool",
  "crimson",
  "dawn",
  "deep",
  "distant",
  "dusty",
  "eager",
  "early",
  "fading",
  "fierce",
  "gentle",
  "golden",
  "hidden",
  "hollow",
  "icy",
  "jolly",
  "keen",
  "lively",
  "lone",
  "lucky",
  "misty",
  "mossy",
  "nimble",
  "patient",
  "quiet",
  "rapid",
  "restless",
  "rustic",
  "silent",
  "silver",
  "sleepy",
  "solar",
  "steady",
  "stormy",
  "sunny",
  "swift",
  "tidy",
  "twilight",
  "velvet",
  "wandering",
  "wild",
  "windy",
}
local NOUNS = {
  "aspen",
  "badger",
  "beacon",
  "birch",
  "brook",
  "canyon",
  "cedar",
  "cliff",
  "comet",
  "coral",
  "cove",
  "creek",
  "delta",
  "dune",
  "ember",
  "falcon",
  "fern",
  "fjord",
  "forest",
  "glacier",
  "grove",
  "harbor",
  "heron",
  "island",
  "lagoon",
  "lantern",
  "maple",
  "meadow",
  "mesa",
  "orchard",
  "otter",
  "pebble",
  "pine",
  "prairie",
  "quarry",
  "raven",
  "reef",
  "ridge",
  "river",
  "summit",
  "thicket",
  "tundra",
  "valley",
  "willow",
  "harvest",
  "cobble",
  "lark",
  "marsh",
  "spruce",
  "wren",
}

---@class dots.Start
local M = {}

---A primary checkout work can start in.
---@class dots.Repo
---@field path string
---@field remotes string[] GitHub `owner/name` of each remote, in config order.

---What a start opens: the worktree for `branch` in `repo`, or the one for
---the PR at `pr`, and failing that a session labelled `label` whose terminal
---runs `argv` in `repo`, or in `cwd` while it clones `repo` first.
---@class dots.StartPlan
---@field repo string
---@field label string
---@field argv string[]
---@field branch? string
---@field pr? string The PR's URL, when its branch is not known locally.
---@field clone? string The GitHub `owner/name` that `argv` clones into `repo` before starting.
---@field cwd? string Where the terminal starts when `repo` does not exist yet.

---@alias dots.StartKind "pr"|"issue"

---@param text string
---@return string
local function trim(text)
  return (text:match("^%s*(.-)%s*$"))
end

---The `wt switch` command that switches to `target`, creating the branch
---when `create` is set, and then runs `run` there: Pi with `prompt`, if any,
---or just the shell. Every
---part is its own argument, so nothing is quoted. Worktrunk expands `{{ }}`
---templates in the arguments after `--`, so a prompt must not contain them;
---starts only pass URLs and issue IDs they have validated.
---@param target string A branch or worktrunk shortcut such as `pr:12`.
---@param create boolean
---@param run "pi"|"shell"
---@param prompt? string Pi's, when `run` is `pi`.
---@return string[]
function M.argv(target, create, run, prompt)
  local argv = { "wt", "switch", "--no-cd" }
  if create then
    argv[#argv + 1] = "--create"
  end
  for _, arg in ipairs({ target, "-x", "zsh", "--", "-ic", SCRIPTS[run], "zsh" }) do
    argv[#argv + 1] = arg
  end
  if prompt then
    argv[#argv + 1] = prompt
  end
  return argv
end

---`argv` run in `path` after cloning GitHub's `slug` there, as one command
---whose parts are all arguments.
---@param slug string
---@param path string Absolute.
---@param argv string[]
---@return string[]
function M.clone_argv(slug, path, argv)
  local out = { "zsh", "-c", CLONE, "zsh", slug, path }
  for _, arg in ipairs(argv) do
    out[#out + 1] = arg
  end
  return out
end

---The `owner/name` a GitHub remote URL names, or nil for any other remote.
---@param url string
---@return string?
function M.github_remote(url)
  local rest = trim(url):gsub("/+$", ""):gsub("%.git$", "")
  for _, prefix in ipairs({
    "git@github.com:",
    "ssh://git@github.com/",
    "https://github.com/",
    "git://github.com/",
  }) do
    if rest:sub(1, #prefix) == prefix then
      local owner, name = rest:sub(#prefix + 1):match("^([^/]+)/([^/]+)$")
      return owner and owner .. "/" .. name or nil
    end
  end
  return nil
end

---The GitHub remotes in a repo's `.git/config`, as written there.
---@param config string
---@return string[]
function M.config_remotes(config)
  local remotes, in_remote = {}, false
  for line in config:gmatch("[^\n]+") do
    local section = line:match("^%s*%[(.-)%]")
    if section then
      in_remote = section:match('^remote%s+"') ~= nil
    elseif in_remote then
      local url = line:match("^%s*url%s*=%s*(.-)%s*$")
      local remote = url and M.github_remote((url:gsub('^"(.*)"$', "%1")))
      if remote then
        remotes[#remotes + 1] = remote
      end
    end
  end
  return remotes
end

---The contents of a file, or nil when it cannot be read.
---@param path string
---@return string?
function M.read(path)
  local file = io.open(path, "r")
  if not file then
    return nil
  end
  local text = file:read("*a")
  file:close()
  return text
end

---The repos among `dirs`: those whose `.git` is a directory, which skips
---linked worktrees and submodules.
---@param dirs string[]
---@param read fun(path: string): string?
---@return dots.Repo[]
function M.known(dirs, read)
  local repos, seen = {}, {}
  for i, dir in ipairs(dirs) do
    if i > DIR_LIMIT then
      break
    end
    -- HEAD is readable only when .git is a directory.
    if not seen[dir] and read(dir .. "/.git/HEAD") then
      repos[#repos + 1] =
        { path = dir, remotes = M.config_remotes(read(dir .. "/.git/config") or "") }
    end
    seen[dir] = true
  end
  return repos
end

---@param repos dots.Repo[]
---@return string
function M.describe(repos)
  if #repos == 0 then
    return "none; zoxide knows no primary checkouts"
  end
  local names = {}
  for _, repo in ipairs(repos) do
    local remote = repo.remotes[1] or "no GitHub remote"
    names[#names + 1] = snapshot.basename(repo.path) .. " (" .. remote .. ")"
  end
  return table.concat(names, ", ")
end

---A GitHub PR URL, which may go on to `/files` or the like or carry a query.
---@param text string
---@return { slug: string, repo: string, number: string, url: string }?
---@return string? err
function M.pr(text)
  local line = trim(text)
  if line == "" then
    return nil, "paste a GitHub PR URL, like " .. PR_FORMS
  end
  local path = line:match("^https://github%.com/([^%s]*)$")
  path = path and path:gsub("[?#].*$", "")
  local owner, repo, number, rest = (path or ""):match("^([^/]+)/([^/]+)/pull/(%d+)(.*)$")
  if
    not owner
    or not owner:match("^[%w%-]+$")
    or not repo:match("^[%w%-_.]+$")
    or number:sub(1, 1) == "0"
    or not (rest == "" or rest:sub(1, 1) == "/")
  then
    return nil, line .. " is not a GitHub PR URL; expected one like " .. PR_FORMS
  end
  local slug = owner .. "/" .. repo
  return {
    slug = slug,
    repo = repo,
    number = number,
    url = "https://github.com/" .. slug .. "/pull/" .. number,
  }
end

---A Linear issue: its ID in upper case, its team, its branch (the lower-case
---ID), and the prompt for Pi (the ID, or the URL without its title slug or
---query).
---@class dots.Issue
---@field id string
---@field team string
---@field branch string
---@field prompt string

---A Linear issue ID in any case, or an issue URL.
---@param text string
---@return dots.Issue?
---@return string? err
function M.issue(text)
  local line = trim(text)
  if line == "" then
    return nil, "type a Linear issue ID or URL, like " .. ISSUE_FORMS
  end
  local workspace, word =
    line:match("^https://linear%.app/([%w%-_]+)/issue/([^/?#%s]+)[/?#]?[^%s]*$")
  local team, number = (word or line):match("^(%a%w*)%-([1-9]%d*)$")
  if not team then
    return nil, line .. " is not a Linear issue; expected an ID or URL like " .. ISSUE_FORMS
  end
  local id = team:upper() .. "-" .. number
  local prompt = workspace and ("https://linear.app/" .. workspace .. "/issue/" .. id) or id
  return { id = id, team = team:lower(), branch = id:lower(), prompt = prompt }
end

---Whether `branch` names `word` as a whole: not preceded by a letter or
---digit, and followed by a digit exactly when `digit` is set. So `abc-12`
---is in `fix/abc-12-login` but not in `abc-123`, and the team `abc-` is in
---`abc-12` but not in `xabc-12`.
---@param branch string
---@param word string
---@param digit boolean
---@return boolean
local function names(branch, word, digit)
  branch = branch:lower()
  local from = 1
  while true do
    local first, last = branch:find(word, from, true)
    if not first then
      return false
    end
    local before = first > 1 and branch:sub(first - 1, first - 1) or ""
    local after = branch:sub(last + 1, last + 1)
    if not before:find("%w") and (after:find("^%d") ~= nil) == digit then
      return true
    end
    from = first + 1
  end
end

---Whether a branch is for the issue.
---@param branch string
---@param issue dots.Issue
---@return boolean
function M.for_issue(branch, issue)
  return names(branch, issue.branch, false)
end

---Whether a branch is for an issue of the team.
---@param branch string
---@param team string Lower case.
---@return boolean
function M.for_team(branch, team)
  return names(branch, team .. "-", true)
end

---The repo to work on an issue in: the one with a branch for it, then the
---one with the most branches for its team. Nil when neither is a single repo.
---@param issue dots.Issue
---@param repos string[] Known repo paths.
---@param branches fun(repo: string): string[]
---@return string?
function M.choose(issue, repos, branches)
  local having, best, most, tied = {}, nil, 0, false
  for _, repo in ipairs(repos) do
    local count, has = 0, false
    for _, branch in ipairs(branches(repo)) do
      has = has or M.for_issue(branch, issue)
      if M.for_team(branch, issue.team) then
        count = count + 1
      end
    end
    if has then
      having[#having + 1] = repo
    end
    if count > most then
      best, most, tied = repo, count, false
    elseif count == most and count > 0 then
      tied = true
    end
  end
  if #having == 1 then
    return having[1]
  end
  if best and not tied then
    return best
  end
  return nil
end

---The local branches of a repo, without `refs/heads/`.
---@param repo string
---@return string[]
function M.branches(repo)
  local result = process.run(
    { "git", "-C", repo, "for-each-ref", "--format=%(refname)", "refs/heads/" },
    { timeout = 5 }
  )
  local out = {}
  if result and result.ok then
    for ref in result.stdout:gmatch("[^\n]+") do
      out[#out + 1] = ref:gsub("^refs/heads/", "")
    end
  end
  return out
end

---Whether the repo has the local branch.
---@param repo string
---@param branch string
---@return boolean
function M.branch_exists(repo, branch)
  local result = process.run(
    { "git", "-C", repo, "show-ref", "--verify", "--quiet", "refs/heads/" .. branch },
    { timeout = 5 }
  )
  return result ~= nil and result.ok
end

---The primary checkout in `git rev-parse --path-format=absolute
-----git-common-dir` output, which is the same from a linked worktree.
---Submodules and bare repos have none.
---@param output string
---@return string?
function M.checkout(output)
  return trim(output):match("^(/.+)/%.git$")
end

---The primary checkout `dir` belongs to, or nil.
---@param dir string
---@return string?
function M.repo_of(dir)
  local result = process.run(
    { "git", "-C", dir, "rev-parse", "--path-format=absolute", "--git-common-dir" },
    { timeout = 5 }
  )
  return result and result.ok and M.checkout(result.stdout) or nil
end

---The known repo with the GitHub remote `slug`, ignoring case: the only
---one, or the one at `at` among several. Nil with no error when none has it.
---@param repos dots.Repo[]
---@param slug string
---@param at? string The checkout path to prefer.
---@return dots.Repo?
---@return string? err
function M.match(repos, slug, at)
  local found = {}
  for _, repo in ipairs(repos) do
    for _, remote in ipairs(repo.remotes) do
      if remote:lower() == slug:lower() then
        found[#found + 1] = repo
        break
      end
    end
  end
  if #found <= 1 then
    return found[1]
  end
  for _, repo in ipairs(found) do
    if repo.path == at then
      return repo
    end
  end
  return nil, slug .. " is the remote of several repos: " .. M.describe(found)
end

---Whether `path` exists as anything but an empty directory.
---@param path string
---@return boolean
function M.occupied(path)
  local listed = process.run({ "ls", "-A", path }, { timeout = 5 })
  return listed ~= nil and listed.ok and listed.stdout ~= ""
end

---@param repo string
---@param what string
---@return string
local function label(repo, what)
  return snapshot.basename(repo) .. ":" .. what
end

---Review a PR in its repo's worktree: in the known repo with its remote,
---preferring the checkout at its placement, else in a clone made there.
---Refuses to clone where something else already is.
---@param pr { slug: string, repo: string, number: string, url: string }
---@param repos dots.Repo[] Including the placement's checkout, if there is one.
---@param placement? dots.Placement
---@param home string
---@return dots.StartPlan?
---@return string? err
function M.plan_pr(pr, repos, placement, home)
  local repo, several = M.match(repos, pr.slug, placement and placement.path)
  if several then
    return nil, several
  end
  local title = pr.repo .. "#" .. pr.number
  local review = M.argv("pr:" .. pr.number, false, "pi", "/skill:review " .. pr.url)
  if repo then
    return { repo = repo.path, label = title, argv = review, pr = pr.url }
  end
  local unmatched = "no known repo has the GitHub remote " .. pr.slug
  local known = "; known: " .. M.describe(repos)
  if not placement then
    return nil,
      unmatched
        .. ", and no rule in "
        .. snapshot.tilde(home .. checkouts.DIR, home)
        .. " places it"
        .. known
  end
  local where = snapshot.tilde(placement.path, home)
    .. " (rule "
    .. placement.key
    .. " in "
    .. snapshot.tilde(placement.file, home)
    .. ")"
  for _, other in ipairs(repos) do
    if other.path == placement.path then
      return nil,
        unmatched
          .. "; its checkout path "
          .. where
          .. " is a checkout of "
          .. (other.remotes[1] or "no GitHub remote")
          .. known
    end
  end
  if M.occupied(placement.path) then
    return nil,
      unmatched
        .. "; not cloning into its checkout path "
        .. where
        .. ", which is not empty and not a checkout"
        .. known
  end
  return {
    repo = placement.path,
    label = title,
    argv = M.clone_argv(pr.slug, placement.path, review),
    pr = pr.url,
    clone = pr.slug,
    cwd = home,
  }
end

---Work on a Linear issue in `repo`, on a branch named for it.
---@param issue dots.Issue
---@param repo string
---@return dots.StartPlan
function M.plan_issue(issue, repo)
  local create = not M.branch_exists(repo, issue.branch)
  return {
    repo = repo,
    label = label(repo, issue.id),
    argv = M.argv(issue.branch, create, "pi", issue.prompt),
    branch = issue.branch,
  }
end

---A generated two-word name, like `twilight-prairie`, that is not among
---`taken`; drawn again until it is not.
---@param taken string[]
---@param random fun(n: integer): integer A whole number from 1 to n, like `math.random`.
---@return string
function M.name(taken, random)
  local used = {}
  for _, branch in ipairs(taken) do
    used[branch] = true
  end
  while true do
    local name = ADJECTIVES[random(#ADJECTIVES)] .. "-" .. NOUNS[random(#NOUNS)]
    if not used[name] then
      return name
    end
  end
end

---Start a new session in the repo of `dir`, the selected row's directory:
---a home worktree with a generated name, with Pi waiting for a prompt.
---@param dir? string
---@param random fun(n: integer): integer
---@return dots.StartPlan?
---@return string? err
function M.plan_new(dir, random)
  local repo = dir and M.repo_of(dir)
  if not repo then
    return nil, "select a repo first: type / and pick one"
  end
  local name = M.name(M.branches(repo), random)
  return {
    repo = repo,
    label = label(repo, name),
    argv = M.argv(name, true, "shell"),
    branch = name,
  }
end

---Known repos: those among `extra` and zoxide's directories.
---@param extra string[] Directories considered first, such as the current repo.
---@return dots.Repo[]
function M.repos(extra)
  local dirs = {}
  for _, dir in ipairs(extra) do
    dirs[#dirs + 1] = dir
  end
  for _, dir in ipairs(snapshot.dirs()) do
    dirs[#dirs + 1] = dir
  end
  return M.known(dirs, M.read)
end

---Plan a PR or issue start from the typed line, or fail with why it is
---invalid. Reads only local git and files.
---@param kind dots.StartKind
---@param line string
---@param here? string The directory of the terminal Go was opened from, whose repo is known even if zoxide does not list it.
---@param home? string Where checkout rules and their `~` are.
---@return dots.StartPlan?
---@return string? err
function M.plan(kind, line, here, home)
  local function known(extra)
    local dirs = { here and M.repo_of(here) or nil }
    if extra then
      dirs[#dirs + 1] = extra
    end
    return M.repos(dirs)
  end
  if kind == "pr" then
    local pr, err = M.pr(line)
    if not pr then
      return nil, err
    end
    if not home or home == "" then
      return nil, "HOME is not set, so checkout rules cannot be read"
    end
    local rules, rules_err = checkouts.load(home, M.read)
    if not rules then
      return nil, rules_err
    end
    local placement = checkouts.resolve(rules, pr.slug, home)
    return M.plan_pr(pr, known(placement and placement.path), placement, home)
  end
  local issue, err = M.issue(line)
  if not issue then
    return nil, err
  end
  local repos = known()
  local paths = {}
  for _, repo in ipairs(repos) do
    paths[#paths + 1] = repo.path
  end
  local repo = M.choose(issue, paths, M.branches)
  if not repo then
    return nil,
      "no repo has a branch for "
        .. issue.branch
        .. " or most branches for team "
        .. issue.team
        .. "; known: "
        .. M.describe(repos)
  end
  return M.plan_issue(issue, repo)
end

---The worktree with `branch` checked out, from `git worktree list
-----porcelain -z` output.
---@param porcelain string NUL-separated fields, each record ending in an empty one.
---@param branch string
---@return string?
function M.worktree_in(porcelain, branch)
  local path
  for field in porcelain:gmatch("([^%z]*)%z") do
    if field == "" then
      path = nil
    elseif field:match("^worktree ") then
      path = field:sub(#"worktree " + 1)
    elseif field == "branch refs/heads/" .. branch then
      return path
    end
  end
  return nil
end

---The worktree of `branch` in `repo`, if it has one.
---@param repo string
---@param branch string
---@return string?
function M.worktree(repo, branch)
  local listing = process.run(
    { "git", "-C", repo, "worktree", "list", "--porcelain", "-z" },
    { timeout = 5 }
  )
  return listing and listing.ok and M.worktree_in(listing.stdout, branch) or nil
end

---The head branch of a PR, which worktrunk checks out under the same name.
---Nil when GitHub cannot be asked.
---@param url string
---@return string?
function M.pr_branch(url)
  local result = process.run(
    { "gh", "pr", "view", url, "--json", "headRefName", "--jq", ".headRefName" },
    { timeout = 10 }
  )
  local branch = result and result.ok and trim(result.stdout) or ""
  return branch ~= "" and branch or nil
end

---The `session.create` arguments for a plan.
---@param plan dots.StartPlan
---@return table
function M.session(plan)
  return {
    label = plan.label,
    initial_windows = {
      {
        layout = rex.layout.block({
          flavor = SHELL,
          options = {
            command = plan.argv,
            cwd = plan.cwd or plan.repo,
            exit = { quick_exit_threshold_ms = FAILURE_WINDOW_MS },
          },
        }),
      },
    },
  }
end

---Go to the session at the plan's worktree, or create one running the plan,
---and record the jump.
---@param ctx rex.Context The picker's.
---@param plan dots.StartPlan
---@param snap dots.Snapshot Without the picker.
function M.open(ctx, plan, snap)
  local from = jumps.here(ctx)
  -- A repo still to be cloned has no worktrees.
  local branch = plan.branch or (plan.pr and not plan.clone and M.pr_branch(plan.pr))
  local path = branch and M.worktree(plan.repo, branch)
  local session, block
  if path then
    session, block = snapshot.session_at(snap, path)
  end
  if session then
    must("focusing the terminal", sessions.focus(session, block --[[@as string]]))
  else
    local created =
      must("creating a session for " .. plan.label, rex.call("session.create", M.session(plan)))
    session = created.session_id
  end
  app.show(session, nil, ctx.session_id, ctx.client_id)
  jumps.record(from, { session_id = session })
end

return M
