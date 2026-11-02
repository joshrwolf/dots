-- Default file row: `[indent] [icon] filename [directory] [decoration] [stats] [status]`.
-- The `[decoration]` segment is the caller's text from codediff.api.set_decorations;
-- a decoration's `name_hl` replaces the filename and directory highlights.
-- The `[stats]` segment renders only when `explorer.line_stats.enabled = true`.

local common = require("codediff.ui.explorer.formatters.common")
local stats = require("codediff.ui.explorer.formatters.stats")

return function(ctx)
  local name_hl = ctx.decoration and ctx.decoration.name_hl
  local left = {
    { segments = common.prefix(ctx) },
    {
      segments = { { text = ctx.filename, hl = name_hl or "Normal" } },
      truncate_priority = 2,
    },
  }
  if ctx.directory ~= "" then
    left[#left + 1] = {
      segments = {
        { text = " ", hl = "Normal" },
        { text = ctx.directory, hl = name_hl or "ExplorerDirectorySmall" },
      },
      truncate_priority = 1,
    }
  end

  local right = {}
  if ctx.decoration and ctx.decoration.text ~= "" then
    right[#right + 1] = {
      segments = { { text = ctx.decoration.text, hl = ctx.decoration.hl or "Normal" }, { text = " ", hl = "Normal" } },
      truncate_priority = 4,
    }
  end
  local file_stats = stats.file_segments(ctx.stats)
  if #file_stats > 0 then
    file_stats[#file_stats + 1] = { text = " ", hl = "Normal" }
    right[#right + 1] = { segments = file_stats, truncate_priority = 3 }
  end
  right[#right + 1] = {
    segments = {
      { text = ctx.status, hl = ctx.status_hl },
      { text = string.rep(" ", ctx.status_right_margin), hl = "Normal" },
    },
  }

  return {
    left = left,
    right = right,
    min_gap = 2,
  }
end
