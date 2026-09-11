local M = {}

function M.target(thread)
  local resolution = thread and thread.resolution
  if type(resolution) == "table" and type(resolution.current_location) == "table" then
    return resolution.current_location
  end
  local anchor = thread and thread.anchor
  if type(anchor) == "table" and anchor.original then
    return anchor.original
  end
  return nil
end

function M.finding(thread)
  return type(thread.finding) == "table" and thread.finding or nil
end

function M.label(thread)
  local finding = M.finding(thread)
  return finding and ("agent " .. (finding.kind or "finding")) or "comment"
end

function M.lines(target)
  if not target then
    return nil, nil
  end
  return target.start_line, target.end_line
end

function M.line(target)
  local first = M.lines(target)
  return first
end

function M.compare(left, right)
  local left_target = M.target(left) or {}
  local right_target = M.target(right) or {}
  if (left_target.path or "") ~= (right_target.path or "") then
    return (left_target.path or "") < (right_target.path or "")
  end
  if (left_target.side or "") ~= (right_target.side or "") then
    return (left_target.side or "") < (right_target.side or "")
  end
  if (M.line(left_target) or 0) ~= (M.line(right_target) or 0) then
    return (M.line(left_target) or 0) < (M.line(right_target) or 0)
  end
  return (left.id or "") < (right.id or "")
end

return M
