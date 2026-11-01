-- Hand-authored `dots github watch list --json` results, decoded, for view
-- tests and the cross-language contract. Neutral names; every state the
-- store can report, including a PR not checked yet and a failed refresh.
local M = {}

local function pr(number, fields)
  local entry = {
    url = "https://github.com/example/project/pull/" .. number,
    repository = "example/project",
    number = number,
    title = "Change " .. number,
    state = "open",
    review = "review_required",
    ci = "pending",
    head_ref = "change-" .. number,
    added_at = 1000 + number,
  }
  for k, v in pairs(fields) do
    entry[k] = v
  end
  return entry
end

---@type dots.WatchList
M.empty = { prs = {}, due = 0 }

---@type dots.WatchList
M.mixed = {
  due = 1,
  prs = {
    pr(1, { state = "unknown", review = "unknown", ci = "unknown" }),
    pr(2, { ci = "failure", checkout = "/home/me/src/dots", rex_session = "session:gone" }),
    pr(3, { review = "changes_requested", ci = "success", rex_session = "session:two" }),
    pr(4, { review = "approved", ci = "success", checked_at = 2000 }),
    pr(5, { state = "draft", review = "none", ci = "none" }),
    pr(6, { error = "Could not resolve to a PullRequest", checkout = "/gone" }),
    pr(7, { state = "merged", review = "approved", ci = "success" }),
    pr(8, { state = "closed", review = "none", ci = "failure" }),
    pr(9, { repository = "other/工作", url = "https://github.com/other/工作/pull/9" }),
  },
}
-- Not checked yet: no title or branch. Lua tables cannot hold nil overrides.
M.mixed.prs[1].title = nil
M.mixed.prs[1].head_ref = nil

return M
