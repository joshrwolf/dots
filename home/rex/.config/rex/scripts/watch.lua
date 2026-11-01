-- Prints a line whenever one of the events in `rex.args.events` happens in any
-- session, and "ready" once it is listening, so its reader can catch up on
-- anything that changed before the subscription began. A live view's picker
-- runs this with `rex do <this file> --all --args '{"events": [...]}'` and
-- reads the view again on every line; it collapses bursts itself.
local events = rex.args.events
if type(events) ~= "table" or #events == 0 then
  error("watch.lua needs a list of events in --args", 0)
end

for _, name in ipairs(events) do
  rex.on(name, function()
    print("changed")
  end)
end

print("ready")
-- Registration may come before Rex starts delivering this script's events, so
-- a change in that gap would go unseen. One more refresh shortly after closes it.
rex.after(1, function()
  print("changed")
end)
