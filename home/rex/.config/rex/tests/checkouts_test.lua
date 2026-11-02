local checkouts = require("dots.checkouts")
local expect = require("tests.expect")
local fake_rex = require("tests.fake_rex")

local HOME = "/home/me"
local DIR = HOME .. "/.config/rex/checkouts"
local DEFAULT = '{ "version": 1, "rules": { "*": "~/dev/personal/{owner}/{name}" } }'

---A rule file's text with `rules`.
---@param rules table<string, string>
---@return string
local function file(rules)
  return assert(rex.json.encode({ version = 1, rules = rules }))
end

---@param files { path: string, text: string }[]
---@return dots.Rules
local function merged(files)
  local rules, err = checkouts.merge(files)
  return assert(rules, err)
end

return {
  ["the most specific rule wins: repo, then owner, then the default"] = function()
    local rules = merged({
      { path = "/a.json", text = DEFAULT },
      {
        path = "/b.json",
        text = file({
          ["Work-Org/*"] = "~/dev/work/{name}",
          ["work-org/side"] = "~/dev/personal/work-org/side",
          ["friend/tool"] = "/srv/tool",
        }),
      },
    })
    local function placed(slug)
      local placement = checkouts.resolve(rules, slug, HOME)
      return placement and { placement.path, placement.key, placement.file }
    end
    expect.equal(placed("work-org/api"), { "/home/me/dev/work/api", "work-org/*", "/b.json" })
    expect.equal(
      placed("WORK-ORG/Side"),
      { "/home/me/dev/personal/work-org/side", "work-org/side", "/b.json" }
    )
    expect.equal(placed("friend/tool"), { "/srv/tool", "friend/tool", "/b.json" })
    expect.equal(placed("friend/other"), { "/home/me/dev/personal/friend/other", "*", "/a.json" })
  end,

  ["templates fill in the owner and name as typed and expand ~"] = function()
    local rules = merged({
      { path = "/a.json", text = file({ ["*"] = "~/src/{owner}-{name}/{name}/" }) },
    })
    expect.equal(
      assert(checkouts.resolve(rules, "My-Org/my_repo.js", HOME)).path,
      "/home/me/src/My-Org-my_repo.js/my_repo.js"
    )
  end,

  ["without a matching rule there is no placement"] = function()
    local rules = merged({ { path = "/a.json", text = file({ ["work-org/*"] = "/w/{name}" }) } })
    expect.equal(checkouts.resolve(rules, "other/tool", HOME), nil)
    expect.equal(checkouts.resolve(merged({}), "other/tool", HOME), nil)
  end,

  ["files merge, and may repeat a rule with the same path"] = function()
    local rules = merged({
      { path = "/a.json", text = DEFAULT },
      {
        path = "/b.json",
        text = file({ ["*"] = "~/dev/personal/{owner}/{name}", ["x/*"] = "/x/{name}" }),
      },
    })
    expect.equal(rules, {
      ["*"] = { path = "~/dev/personal/{owner}/{name}", file = "/a.json" },
      ["x/*"] = { path = "/x/{name}", file = "/b.json" },
    })
  end,

  ["two files placing one key differently is an error naming both"] = function()
    expect.equal({
      checkouts.merge({
        { path = "/a.json", text = file({ ["Work-Org/*"] = "~/dev/work/{name}" }) },
        { path = "/b.json", text = file({ ["work-org/*"] = "~/work/{name}" }) },
      }),
    }, {
      nil,
      "checkout rule work-org/* conflicts: /a.json says ~/dev/work/{name}, /b.json says ~/work/{name}",
    })
  end,

  ["a malformed file is an error naming the file and key"] = function()
    for _, case in ipairs({
      { "not json", "/a.json: not JSON" },
      { "{}", "/a.json: version must be 1, not nil" },
      { '{ "version": 2, "rules": {} }', "/a.json: version must be 1, not 2" },
      { '{ "version": 1 }', "/a.json: rules must be an object of KEY: PATH" },
      {
        '{ "version": 1, "rules": {}, "extra": true }',
        "/a.json: unknown field extra; expected version and rules",
      },
      {
        file({ ["owner"] = "/x" }),
        "/a.json: rule owner must be owner/name, owner/*, or *",
      },
      {
        file({ ["*/name"] = "/x" }),
        "/a.json: rule */name must be owner/name, owner/*, or *",
      },
      {
        file({ ["owner/*"] = "dev/{name}" }),
        "/a.json: rule owner/* must be a path starting with ~/ or / that uses only {owner} and {name}",
      },
      {
        file({ ["owner/*"] = "~/dev/{repo}" }),
        "/a.json: rule owner/* must be a path starting with ~/ or / that uses only {owner} and {name}",
      },
      {
        file({ ["Owner/*"] = "/a/{name}", ["owner/*"] = "/b/{name}" }),
        "/a.json: rule owner/* is given twice with different paths",
      },
    }) do
      local rules, err = checkouts.merge({ { path = "/a.json", text = case[1] } })
      expect.equal({ case[1], rules, err and err:sub(1, #case[2]) }, { case[1], nil, case[2] })
    end
  end,

  ["rules load from every JSON file in the rule directory"] = function(t)
    fake_rex.install(t, {
      commands = {
        ["ls -1 " .. DIR] = { stdout = "work.json\nREADME.md\ndefault.json\n" },
      },
    })
    local texts = {
      [DIR .. "/default.json"] = DEFAULT,
      [DIR .. "/work.json"] = file({ ["work-org/*"] = "~/dev/work/{name}" }),
    }
    local rules = assert(checkouts.load(HOME, function(path)
      return texts[path]
    end))
    expect.equal(rules, {
      ["*"] = { path = "~/dev/personal/{owner}/{name}", file = DIR .. "/default.json" },
      ["work-org/*"] = { path = "~/dev/work/{name}", file = DIR .. "/work.json" },
    })
    expect.equal(
      { checkouts.load(HOME, function() end) },
      { nil, DIR .. "/default.json: cannot be read" }
    )
  end,

  ["a missing rule directory has no rules"] = function(t)
    fake_rex.install(t, { commands = { ["ls -1 " .. DIR] = { ok = false } } })
    expect.equal(checkouts.load(HOME, function() end), {})
  end,
}
