local M = {}

-- Review-owned virtual lines do not inherit window 'wrap'. Split only our
-- prose, measuring terminal cells and keeping combining characters together.
function M.lines(text, width)
  width = math.max(1, width)
  local lines, line = {}, ""
  for word in text:gmatch("%S+") do
    if line ~= "" and vim.fn.strdisplaywidth(line .. " " .. word) <= width then
      line = line .. " " .. word
    else
      if line ~= "" then lines[#lines + 1] = line end
      line = ""
      local offset = 0
      while offset < vim.fn.strchars(word, true) do
        local char = vim.fn.strcharpart(word, offset, 1, true)
        if line ~= "" and vim.fn.strdisplaywidth(line .. char) > width
          and char ~= "‍" and line:sub(-#"‍") ~= "‍" then
          lines[#lines + 1] = line
          line = ""
        end
        line = line .. char
        offset = offset + 1
      end
    end
  end
  if line ~= "" or #lines == 0 then lines[#lines + 1] = line end
  return lines
end

return M
