-- Hand-authored snapshots for view tests and the cross-language contract.
-- Neutral names only; cover what real sessions can contain, including what
-- this machine has not produced yet.
local M = {}

---@type dots.Snapshot
M.empty = { sessions = {}, worktrees = {}, dirs = {}, home = "/home/me" }

---@type dots.Snapshot
M.busy = {
  home = "/home/me",
  sessions = {
    {
      id = "session:one",
      label = "dots",
      blocks = {
        {
          id = "block:shell",
          window_id = "window:1",
          tab = 1,
          cwd = "/home/me/src/dots",
        },
        {
          id = "block:pi",
          window_id = "window:1",
          tab = 1,
          program = "pi",
          focused = true,
          cwd = "/home/me/src/dots",
          status = {
            state = "working",
            app = "pi",
            msg = "editing",
            updated_at = "2026-10-20T10:00:00Z",
          },
        },
        {
          id = "block:done",
          window_id = "window:2",
          tab = 2,
          cwd = "/home/me/src/dots",
          status = {
            state = "done",
            effective_app = "claude-code",
            title = "Refactor ✓",
            msg = "finished",
          },
        },
        {
          id = "block:worker",
          window_id = "window:5",
          tab = 3,
          tab_label = "review",
          label = "review",
          program = "pi",
          cwd = "/home/me/.wt/home/me/src/dots/fix-login",
          status = { state = "working", app = "pi", msg = "1 worker" },
        },
        {
          id = "block:build",
          window_id = "window:6",
          tab = 4,
          program = "cargo",
          cwd = "/home/me/.wt/home/me/src/dots/fix-login/src",
          status = {
            state = "working",
            app = "cd",
            msg = "cd /home/me/.wt/home/me/src/dots/fix-login && FAST=1 cargo build --manifest-path src/Cargo.toml",
          },
        },
      },
    },
    {
      id = "session:two",
      label = "café · 👩‍💻",
      blocks = {
        {
          id = "block:blocked",
          window_id = "window:3",
          tab = 1,
          cwd = "/home/me/src/with space\tand tab",
          status = {
            state = "blocked",
            kind = "permission",
            app = "codex",
            msg = "approve rm -rf build?",
          },
        },
        {
          id = "block:failed",
          window_id = "window:3",
          tab = 1,
          status = { state = "error", app = "cargo", msg = "build failed\nsecond line" },
        },
        {
          id = "block:idle",
          window_id = "window:4",
          tab = 2,
          program = "nvim",
          status = { state = "idle", app = "pi" },
        },
      },
    },
    { id = "session:empty", label = "empty", blocks = {} },
  },
  worktrees = {
    "/home/me/src/dots",
    "/home/me/src/dots-feature",
    "/home/me/.wt/home/me/src/dots/fix-login",
    "/elsewhere/工作",
  },
  dirs = { "/home/me/src/dots-feature", "/tmp", "/home/me" },
}

return M
