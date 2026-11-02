-- Drop-in replacement for nui.line
-- Provides styled line construction with highlighted segments

local Line = {}
Line.__index = Line

function Line.new()
  return setmetatable({ _segments = {} }, Line)
end

-- Allow Line() constructor syntax
setmetatable(Line, {
  __call = function(cls)
    return cls.new()
  end,
})

--- Append a text segment with an optional highlight group
---@param text string
---@param hl_group? string
---@return Line self for chaining
function Line:append(text, hl_group)
  self._segments[#self._segments + 1] = { text = text or "", hl = hl_group }
  return self
end

--- Get the full text content of all segments concatenated
---@return string
function Line:content()
  local parts = {}
  for _, seg in ipairs(self._segments) do
    parts[#parts + 1] = seg.text
  end
  return table.concat(parts)
end

return Line
