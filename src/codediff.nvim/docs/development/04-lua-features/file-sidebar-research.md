# File Sidebar Research: Neo-tree Integration

## Executive Summary

**Recommendation: Use Neo-tree.nvim with a custom source for full flexibility**

Neo-tree.nvim provides a modern, extensible file sidebar for Neovim (~5,000 stars, active maintenance, no-breaking-changes policy). Its built-in `git_status` source covers basic working directory comparisons, but for advanced use cases—comparing arbitrary commits, integrating with a C diff engine, showing inline diff stats—a **custom Neo-tree source** gives complete control while leveraging all of Neo-tree's UI infrastructure.

## Why Neo-tree?

### 1. Already Has What You Need ✅

Neo-tree comes with a built-in `git_status` source that:
- Shows all modified, added, deleted, untracked files
- Displays git status icons (M, A, D, ??, etc.)
- Tree structure for organized directory view
- Built-in commands: git add, unstage, revert, commit, push
- Multiple view modes: sidebar, floating window, netrw-style

### 2. Modern Architecture ✅

- Built on **nui.nvim** (the best Neovim UI library)
- Component-based rendering system
- Public API and event system
- Extensible source architecture
- Async file watching with libuv

### 3. Beautiful Appearance ✅

- Git status colors and icons
- File icons via nvim-web-devicons
- Indent guides
- Smooth animations
- Customizable components
- Multiple layout options

### 4. Performance ✅

- Async operations (non-blocking)
- Efficient rendering
- Smart refresh on git events
- File system watching

## Comparison with Alternatives

### nvim-tree.lua
- **Stars**: ~8,000
- **Status**: Feature-frozen (no new major features)
- **Verdict**: ❌ Not recommended - less extensible, no new features

### Custom Implementation
- **Control**: Full control
- **Effort**: High - reinventing the wheel
- **Verdict**: ❌ Not recommended - use Neo-tree instead

### Neo-tree Custom Source vs Building from Scratch

| Feature | Neo-tree Custom Source | From Scratch |
|---------|----------------------|--------------|
| Tree rendering | ✅ Built-in | ❌ Need to implement |
| File icons | ✅ Built-in | ❌ Need to implement |
| Keyboard navigation | ✅ Built-in | ❌ Need to implement |
| Floating windows | ✅ Built-in | ❌ Need to implement |
| Customization | ✅ Full control | ✅ Full control |
| Effort | 🟡 Medium | 🔴 High |
| Maintenance | ✅ Neo-tree handles UI | ❌ You handle everything |

## Built-in git_status: Capabilities and Limitations

### What it CAN do:
```vim
:Neotree git_status                    " Working dir vs HEAD
:Neotree git_status git_base=main      " Working dir vs main branch
:Neotree git_status git_base=HEAD~1    " Working dir vs HEAD~1
```

### What it CANNOT do:
- ❌ Compare two arbitrary commits (e.g., `abc123` vs `def456`)
- ❌ Show diff stats inline
- ❌ Custom filtering
- ❌ Integration with your C diff engine

### Why?
Looking at the code (`lua/neo-tree/git/status.lua`):

```lua
-- Built-in uses these git commands:
git diff --staged --name-status <base> --
git diff --name-status                    -- unstaged changes
git ls-files --exclude-standard --others  -- untracked files
```

It's designed to show **working directory status**, not arbitrary commit comparisons.

## Custom Source: Full Flexibility ✅

### Source API Structure

```lua
-- Location: lua/neo-tree/sources/vscode_diff/init.lua
local M = {
  name = "vscode_diff",           -- Required
  display_name = " 󰊢 VSCode Diff ", -- Required
}

-- FULL CONTROL HERE
function M.navigate(state, path, path_to_reveal, callback, async)
  -- 1. Get ANY data you want
  -- 2. Build tree structure YOUR way
  -- 3. Call YOUR C diff engine
  -- 4. Show YOUR custom components
end

function M.setup(config, global_config)
  -- Configure YOUR source
  -- Subscribe to events
  -- Setup YOUR custom settings
end

return M
```

### Example: Compare ANY Two Commits

```lua
-- Custom function using git diff
local function get_files_between_commits(commit1, commit2, cwd)
  local result = vim.system({
    "git",
    "diff",
    "--name-status",  -- M, A, D status
    commit1,
    commit2
  }, { cwd = cwd, text = true }):wait()

  if result.code ~= 0 then
    return {}
  end

  local files = {}
  for _, line in ipairs(vim.split(result.stdout, "\n")) do
    if line ~= "" then
      local status, path = line:match("^(%S+)%s+(.+)$")
      if status and path then
        files[path] = status
      end
    end
  end

  return files
end

function M.navigate(state, path, path_to_reveal, callback, async)
  -- Get custom parameters from user
  local commit1 = state.commit1 or "HEAD~1"
  local commit2 = state.commit2 or "HEAD"

  -- Get files YOUR way
  local status_lookup = get_files_between_commits(commit1, commit2, state.path)

  -- Build tree and render
  -- (see full example below)
end
```

### Usage Examples

```vim
" Compare any two commits
:Neotree vscode_diff commit1=HEAD~5 commit2=HEAD
:Neotree vscode_diff commit1=main commit2=feature-branch
:Neotree vscode_diff commit1=abc123 commit2=def456

" Floating window
:Neotree float vscode_diff commit1=v1.0.0 commit2=v2.0.0
```

## Advanced Customization Possibilities

### 1. Custom Components (Show Diff Stats)

```lua
-- lua/neo-tree/sources/vscode_diff/components.lua
local M = {}

M.diff_stats = function(config, node, state)
  if node.type ~= "file" then
    return {}
  end

  -- Get diff stats using YOUR C diff engine
  local stats = get_diff_stats(node.path, state.commit1, state.commit2)

  return {
    text = string.format("+%d -%d", stats.additions, stats.deletions),
    highlight = "DiffAdd",
  }
end

return M
```

Then use it in your renderer:

```lua
-- In your source setup
window = {
  mappings = {
    -- Custom keybindings
    ["<CR>"] = function(state)
      local node = state.tree:get_node()
      -- Open in vscode-diff
      vim.cmd("CodeDiff " .. state.commit1)
    end,
  }
},
renderers = {
  file = {
    { "icon" },
    { "name", use_git_status_colors = true },
    { "diff_stats" },  -- YOUR custom component
    { "git_status" },
  },
}
```

### 2. Integration with C Diff Engine

```lua
function M.navigate(state, path, path_to_reveal, callback, async)
  local files = get_files_between_commits(commit1, commit2, state.path)

  -- For each file, compute diff stats with YOUR C engine
  for path, status in pairs(files) do
    local full_path = state.path .. "/" .. path

    -- Call YOUR C diff function via FFI
    local diff_stats = compute_diff_with_c_engine(
      get_file_at_commit(commit1, path),
      get_file_at_commit(commit2, path)
    )

    -- Store in node.extra
    item.extra = {
      git_status = status,
      diff_stats = diff_stats,  -- YOUR custom data
      commit1 = commit1,
      commit2 = commit2,
    }
  end
end
```

### 3. Custom Filtering

```lua
function M.navigate(state, path, path_to_reveal, callback, async)
  local files = get_files_between_commits(commit1, commit2, state.path)

  -- FILTER as you want
  if state.filter_mode == "modified_only" then
    files = vim.tbl_filter(function(path, status)
      return status == "M"
    end, files)
  end

  if state.file_pattern then
    files = vim.tbl_filter(function(path, status)
      return path:match(state.file_pattern)
    end, files)
  end

  -- Build tree from filtered files
end
```

### 4. Custom Sorting

```lua
-- Sort by number of changes
file_items.advanced_sort(root.children, state, function(a, b)
  local a_stats = a.extra.diff_stats or {}
  local b_stats = b.extra.diff_stats or {}

  local a_changes = (a_stats.additions or 0) + (a_stats.deletions or 0)
  local b_changes = (b_stats.additions or 0) + (b_stats.deletions or 0)

  return a_changes > b_changes
end)
```

## Full Example: Custom Source

```lua
-- lua/neo-tree/sources/vscode_diff/init.lua
local M = {
  name = "vscode_diff",
  display_name = " 󰊢 VSCode Diff ",
}

local renderer = require("neo-tree.ui.renderer")
local file_items = require("neo-tree.sources.common.file-items")

-- Get files between ANY two commits
local function get_files_between_commits(commit1, commit2, cwd)
  local result = vim.system({
    "git", "diff", "--name-status", commit1, commit2
  }, { cwd = cwd, text = true }):wait()

  if result.code ~= 0 then
    return {}
  end

  local files = {}
  for _, line in ipairs(vim.split(result.stdout, "\n")) do
    if line ~= "" then
      local status, path = line:match("^(%S+)%s+(.+)$")
      if status and path then
        files[path] = status
      end
    end
  end
  return files
end

function M.navigate(state, path, path_to_reveal, callback, async)
  state.path = path or state.path or vim.fn.getcwd()

  -- Get custom parameters
  local commit1 = state.commit1 or "HEAD~1"
  local commit2 = state.commit2 or "HEAD"

  -- Get changed files
  local status_lookup = get_files_between_commits(commit1, commit2, state.path)

  -- Build tree
  local context = file_items.create_context()
  context.state = state

  local root = file_items.create_item(context, state.path, "directory")
  root.name = string.format("%s...%s", commit1, commit2)
  root.loaded = true
  context.folders[root.path] = root

  -- Add files
  for path, status in pairs(status_lookup) do
    local full_path = state.path .. "/" .. path
    local success, item = pcall(file_items.create_item, context, full_path, "file")
    if success then
      item.status = status
      item.extra = {
        git_status = status,
        commit1 = commit1,
        commit2 = commit2,
      }
    end
  end

  -- Expand and render
  state.default_expanded_nodes = {}
  for id, _ in pairs(context.folders) do
    table.insert(state.default_expanded_nodes, id)
  end

  renderer.show_nodes({ root }, state)

  if type(callback) == "function" then
    vim.schedule(callback)
  end
end

function M.setup(config, global_config)
  -- Add custom commands, events, etc.
end

return M
```

Usage:

```vim
:Neotree vscode_diff commit1=HEAD~5 commit2=HEAD
:Neotree float vscode_diff commit1=main commit2=develop
```

## Implementation Guide

### Install Neo-tree

```lua
-- Using lazy.nvim
{
  "nvim-neo-tree/neo-tree.nvim",
  branch = "v3.x",
  dependencies = {
    "nvim-lua/plenary.nvim",
    "MunifTanjim/nui.nvim",
    "nvim-tree/nvim-web-devicons", -- optional but recommended
  }
}
```

### Basic Usage

```vim
:Neotree git_status           " Open in sidebar
:Neotree float git_status     " Open in floating window
:Neotree git_status git_base=main  " Compare with branch
```

### Integration with vscode-diff.nvim

#### Option 1: Simple Command Integration

Add this to your `plugin/vscode-diff.lua`:

```lua
-- Command to show git status list
vim.api.nvim_create_user_command("CodeDiffList", function()
  vim.cmd("Neotree float git_status")
end, { desc = "Show git changed files" })
```

Users can then:
1. Run `:CodeDiffList` to see changed files
2. Navigate to desired file in Neo-tree
3. Press Enter to edit the file
4. Run `:CodeDiff HEAD` to view the diff

#### Option 2: Custom Keybinding Integration

Configure Neo-tree to open files directly in vscode-diff:

```lua
-- In user's Neo-tree config
require("neo-tree").setup({
  git_status = {
    window = {
      mappings = {
        -- Press 'dd' on a file to open it in vscode-diff
        ["dd"] = function(state)
          local node = state.tree:get_node()
          if node and node.type == "file" then
            -- Close neo-tree
            vim.cmd("Neotree close")
            -- Open the file
            vim.cmd("edit " .. node.path)
            -- Open diff with HEAD
            vim.cmd("CodeDiff HEAD")
          end
        end,

        -- Or press 'd1' for HEAD~1
        ["d1"] = function(state)
          local node = state.tree:get_node()
          if node and node.type == "file" then
            vim.cmd("Neotree close")
            vim.cmd("edit " .. node.path)
            vim.cmd("CodeDiff HEAD~1")
          end
        end,
      }
    }
  }
})
```

### Advanced: Custom Neo-tree Source (Future Enhancement)

Directory structure for a custom source:

```
vscode-diff.nvim/
└── lua/
    └── neo-tree/
        └── sources/
            └── vscode_diff/
                ├── init.lua          # Main source
                ├── lib/
                │   └── items.lua     # Get file list with your git module
                └── components.lua    # Custom rendering
```

## Recommended Implementation Path

**Phase 1: Start Simple**
1. Document that users should install Neo-tree
2. Add `:CodeDiffList` command that opens Neo-tree git_status
3. Document the workflow in your README

**Phase 2: Custom Source**
1. Create a basic custom source to compare two arbitrary commits
2. Basic tree rendering with git status
3. Open files directly in vscode-diff on Enter

**Phase 3: Enhanced Components**
1. Show diff stats inline via custom components
2. Add file size changes and color coding based on change amount
3. Add example Neo-tree configuration to your docs

**Phase 4: Advanced Integration**
1. Integration with C diff engine for diff stats
2. Custom filtering/sorting
3. Keybindings for common workflows
4. Preview diff stats in statusline

## Documentation Example for README

```markdown
## Git Changed Files List

To see a list of all git changed files with a beautiful sidebar, we recommend
using [Neo-tree.nvim](https://github.com/nvim-neo-tree/neo-tree.nvim)'s
built-in `git_status` source.

### Quick Start

1. Install Neo-tree.nvim (see their installation docs)

2. Use the built-in command:
   ```vim
   :Neotree git_status
   ```

3. Navigate to a file and press Enter to edit it, then run:
   ```vim
   :CodeDiff HEAD
   ```

### Convenience Command

Add this to your config for quick access:
```lua
vim.api.nvim_create_user_command("CodeDiffList", function()
  vim.cmd("Neotree float git_status")
end, {})
```

Now you can run `:CodeDiffList` to quickly see all changed files!
```

## Conclusion

**Use Neo-tree.nvim with a custom source** — it's the modern, performant, beautiful solution that follows Neovim best practices. The custom source system gives complete control over data retrieval and rendering while Neo-tree handles all UI infrastructure.

- ✅ Already feature-complete for basic needs via built-in `git_status`
- ✅ Fully customizable via custom source for advanced use cases
- ✅ Actively maintained (~5,000 stars, no-breaking-changes policy)
- ✅ Modern architecture (nui.nvim based, component system, event system)
- ✅ Can integrate with C diff engine, custom filtering/sorting
- ✅ Extensible with custom components for inline diff stats
