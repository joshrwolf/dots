-- Every view the Rex configuration builds, from the hand-authored snapshots,
-- for src/pick/tests/contract.rs to check against the dots-pick format.
-- Run: rex --autostart=false do tests/contract.lua (tests/isolate.lua keeps it off
-- any running server; the builders are pure).
local source = debug.getinfo(1, "S").source:gsub("^@", "")
local root = source:match("^(.*)/tests/contract%.lua$") or "."
package.path = root .. "/?.lua;" .. root .. "/?/init.lua;" .. package.path
require("tests.isolate").install()

local go = require("dots.go")
local prs = require("dots.prs")
local snapshot = require("dots.snapshot")
local snapshots = require("tests.fixtures.snapshots")
local watch = require("tests.fixtures.watch")
local views = require("dots.views")

local out = {}
for name, snap in pairs(snapshots) do
  out["go/" .. name] = views.live(go.view(snap, nil, "studio"), snapshot.events({ places = true }))
  for list_name, list in pairs(watch) do
    out["prs/" .. list_name .. "/" .. name] = prs.view(list, snap, "dots")
  end
end
return out
