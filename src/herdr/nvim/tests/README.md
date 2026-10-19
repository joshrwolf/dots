# Editor bridge tests

Use existing frameworks; keep our harness specific to process ownership and
Herdr fixtures. Do not add a homegrown assertion library or test runner.

## Commands

From the dots root:

```sh
mise run //src/herdr:deps  # explicit network operation; restore Neovim's committed package lock
mise run rust-check     # offline full workspace gate, including these tests
```

For the editor layer only:

```sh
cd src/herdr
cargo nextest run -p herdr-nvim --test editor --success-output immediate
```

This reports mini.test's named cases as well as the Rust contract tests. No
runtime plugin installation is needed. A missing or modified mini.test checkout
fails with an actionable error instead of downloading during tests. To update,
use Neovim's `vim.pack` API with the isolated config/data paths declared in the
dependency task, and review `nvim/nvim-pack-lock.json` and the suite against that
version. The normal dependency task restores the committed lock, not a floating branch. The upstream repo/license stay in the
cached checkout; no copied framework code is maintained here.

## Three layers

1. **Rust unit tests**: discovery, selection, typed Herdr operations, protocol
   decoding, bounded subprocesses, private state storage and recovery intent.
   Herdr wire tests use the shared
   `herdrkit::testing::Server`, not another socket fake.
2. **Lua behavior tests**: mini.test collection, parameterization, expectations,
   reporting, and child-process control. `lua/support.lua` starts a fresh child
   for each case, with both per-case and suite cleanup. These tests exercise
   real Neovim buffers/windows/autocommands and a narrow fake host adapter.
3. **Cross-language tests**: Rust serializes the production `protocol::Request`
   types into real native Neovim RPC. Real Lua replies deserialize into Rust's
   `Reply` variants. Invalid cases mutate those Rust-produced messages, rather
   than maintaining an unrelated collection of supposedly valid JSON snapshots.

`fixtures/editor.lua` is only a server fixture. It contains no test suite and
has no hidden "run tests vs serve" switch. It composes the real listener,
endpoint, protocol, dispatcher and editor without any pane publication. It does
not fake successful registration to make a headless editor ready. Its small
bootstrap/control surface exists only in the fixture. `support/mod.rs` owns the child,
deadline, temporary files and cleanup; application code has no test mode.

The full navigation test also uses the real Rust transport and the shared Herdr
fake to assert the exact list/get/focus requests, typed rejection after movement,
and unchanged buffer/cursor. An unreachable registered caller beside a real live
sibling proves there is no fallback. A SIGSTOP/timeout/SIGCONT test proves an
actually delivered queued request expires rather than navigating late. Serde's
actual enum variant names are compared with Lua's exported error-code table.
The host adapter has its own tests for exact CLI arguments and asynchronous
lookup/report ordering. It accepts a process executor at that seam; tests never
replace `vim.system`, `uv.random`, buffer APIs or globals used by other modules.

## Lifecycle tests

`fake_host.lua` does not immediately pretend an async request finished. Tests
explicitly commit and deliver publication once, deliver callbacks after shutdown,
and arrange an in-flight report to finish while shutdown waits. Process fakes
invoke each completion only once, including when `wait()` follows completion. Assertions cover
both phase and side-effect order. Host adapter tests separately check that a
queued lookup cannot launch publication after cancellation. Local readiness is
independent of publication: tests cover failed/withdrawn advertisements, closed
endpoints, fixed server association, and listener cleanup failures.

Editor tests distinguish preparation from presentation. `bufload` itself can
run user hooks, so the partial-presentation case preloads the target before
injecting its BufEnter failure. A rejected request does not promise that no
BufRead hook ran. Contract failures assert stable codes, never diagnostic prose.
Two live editors test swap-conflict retries; a separate case guards the loaded
but conflicted buffer path. mini.test adds `-n`, so the harness explicitly resets
`updatecount`/`swapfile` to their defaults after startup, using a private shared
swap directory. Swap safety is not tested with swap behavior disabled.

UI eligibility uses a real native remote-UI client in a disposable PTY, not a
patched `nvim_list_uis`. Special-window cases cover nofile/help/quickfix/terminal
and floating windows without needing Oil or CodeDiff to be installed.

`test_ui.lua` makes the native ownership assumptions executable: UI termination,
`:detach`, reattachment with visible/hidden dirty buffers and undo/register state,
multiple-UI shared focus, and the distinction between detach and editor quit.
`support.ui` owns each native client and registers cleanup immediately. These
are lifecycle tests, not a claim that a persistent controller is implemented.

`test_ownership.lua` tests editor-side controller leases with a narrow monotonic
clock seam, plus real elapsed-time expiry. It verifies stale-owner rejection,
explicit competing recovery, idempotent release, non-renewing recovery replay,
and fencing again after buffer hooks. Rust contract cases serialize the actual
owner DTOs and exercise them through native RPC. The fixture can opt into managed
mode only before requests begin; this is not an automatic ownership upgrade or
a launched controller service.

The recovery-ledger contract test persists identity before starting the native
editor, then reopens unfinished launch and recovery records. It reconciles the
exact observed generation without renewing it. Listener cases verify that a
reserved identity is used exactly and that existing socket/file paths are not
removed to make startup succeed. Storage tests inject failures around write,
sync and rename boundaries; a Python stdlib `fcntl` probe checks cross-process
exclusion and abrupt-exit lock release. The non-regular-file test uses POSIX
`mkfifo`. These do not simulate hardware power loss or a deployed controller.

## Isolation and limitations

All editors use minimal startup, no shada, and temporary log/state/cache paths.
The child bootstrap removes live Herdr caller context. Framework/library setup
never reads user config. A parent deadline bounds the suite, and the shared
subprocess runner kills its process group on failure. Fixtures use random owned
listener names in isolated private directories, never `/tmp/dots-nvim-<uid>`.
They remove only their own resources. The listener and Rust discovery accept
explicit directories without environment-based test modes.

These tests intentionally do not claim to validate the installed Herdr CLI
against a live server, a full user configuration, clipboard/terminal behavior,
or sidebar layouts. Those remain explicit, disposable-workspace acceptance
checks when those layers change. Never turn a live session capture into a fixture.
