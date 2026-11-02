# Git Integration

The plugin provides built-in git integration to compare the current buffer with any git revision.

## Usage

### Basic Git Diff

When you have a file open that's tracked by git, you can compare it with any revision:

```vim
:CodeDiff HEAD
```

This will:
1. Validate that the current buffer is a file in a git repository
2. Asynchronously fetch the file content from the specified git revision
3. Open a new tab with a side-by-side diff view:
   - **Left buffer**: File content at the specified git revision (readonly)
   - **Right buffer**: Current buffer content (readonly)

### Supported Git Revisions

The command supports all standard git revision formats:

```vim
" HEAD and relatives
:CodeDiff HEAD          " Last commit
:CodeDiff HEAD~1        " One commit before HEAD
:CodeDiff HEAD~5        " Five commits before HEAD
:CodeDiff HEAD^         " First parent of HEAD

" Commit hashes
:CodeDiff abc123        " Short hash
:CodeDiff abc123def456  " Full hash

" Branches
:CodeDiff main
:CodeDiff develop
:CodeDiff feature/my-branch

" Tags
:CodeDiff v1.0.0
:CodeDiff release-2024

" Special refs
:CodeDiff origin/main
:CodeDiff @{upstream}
```

## Error Handling

The plugin provides clear error messages for common issues:

### Not in a Git Repository

```vim
:CodeDiff HEAD
" Error: Current file is not in a git repository
```

### Current Buffer Not a File

```vim
" In a scratch buffer or empty buffer
:CodeDiff HEAD
" Error: Current buffer is not a file
```

### File Not in Revision

```vim
:CodeDiff HEAD~10
" Error: File 'path/to/file.lua' not found in revision 'HEAD~10'
```

This can happen if:
- The file didn't exist in that revision
- The file was added after that revision
- The file was renamed (rename detection not yet implemented)

### Invalid Revision

```vim
:CodeDiff nonexistent-branch
" Error: Invalid revision 'nonexistent-branch': ...
```

## Lua API

### Check if File is in Git Repository

```lua
local git = require("vscode-diff.git")

if git.is_in_git_repo(vim.api.nvim_buf_get_name(0)) then
  print("Current file is in a git repository")
end
```

### Get Git Root

```lua
local git = require("vscode-diff.git")

local git_root = git.get_git_root("/path/to/file.lua")
if git_root then
  print("Git root:", git_root)
end
```

### Get File from Git Revision (Async)

```lua
local git = require("vscode-diff.git")

git.get_file_at_revision("HEAD~1", "/path/to/file.lua", function(err, lines)
  if err then
    vim.notify(err, vim.log.levels.ERROR)
    return
  end
  
  -- lines is a table of strings (file lines)
  print("Got", #lines, "lines from git")
  
  -- You can now compare with current buffer
  local current_lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
  local diff = require("vscode-diff")
  local plan = diff.compute_diff(lines, current_lines)
  
  local render = require("vscode-diff.render")
  render.render_diff(lines, current_lines, plan)
end)
```

### Validate Revision

```lua
local git = require("vscode-diff.git")

git.validate_revision("main", "/path/to/file.lua", function(err)
  if err then
    print("Invalid revision:", err)
  else
    print("Revision is valid")
  end
end)
```

## Implementation Details

### Async Operations

All git operations that fetch file content are asynchronous to prevent blocking Neovim. The plugin uses:

- `vim.system()` on Neovim 0.10+ (preferred)
- `vim.loop.spawn()` on older versions (fallback)

### Git Commands Used

The plugin uses standard git commands:

- `git rev-parse --show-toplevel` - Get repository root
- `git show <revision>:<path>` - Get file content from a revision
- `git rev-parse --verify <revision>` - Validate a revision exists

### Performance

- Repository detection is done synchronously (fast)
- File content fetching is done asynchronously (non-blocking)
- The diff computation happens after file content is retrieved

## File History with Fixed Base (`--base`)

By default, `:CodeDiff history` compares each commit against its parent.
The `--base` flag changes this to compare every commit against a fixed reference:

```vim
" Compare each commit against the current working tree
:CodeDiff history --base WORKING

" Compare each commit against HEAD
:CodeDiff history --base HEAD

" Compare each commit against a branch
:CodeDiff history --base main

" Short form
:CodeDiff history -b WORKING

" Combine with range, file path, and other flags
:CodeDiff history origin/main..HEAD --base WORKING %
:CodeDiff history HEAD~10 -b HEAD --reverse
```

### Comparison Modes

**Sequential (default):** Each commit compared to its parent.
Useful for reviewing what changed in each individual commit.

**Fixed base (`--base WORKING`):** Each commit compared to working tree.
Useful for seeing how far each historical commit is from your current state.

**Fixed base (`--base <revision>`):** Each commit compared to a specific ref.
Useful for seeing how each commit differs from a branch tip or tag.

## Future Enhancements

Potential improvements:

- [ ] Rename detection with `git log --follow`
- [ ] Support for comparing between two arbitrary revisions
- [ ] Staged vs working directory comparison
- [ ] Conflict resolution view for merge conflicts
- [ ] Integration with other git operations (stage hunks, etc.)
