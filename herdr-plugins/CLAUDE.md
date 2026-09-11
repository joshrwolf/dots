# herdr-plugins design constitution

- Treat this directory as one Cargo workspace: `herdrkit`, contract tests, and one crate per plugin.
- Load the `herdr-plugin` and `idiomatic-rust` skills for changes here; the former owns Herdr protocol semantics and the latter owns Rust design.
- Keep protocol wire shapes, validation, environment parsing, transport, terminal UI primitives, and test fakes in `herdrkit`.
- Plugins must use typed `herdrkit::Client` operations; do not add raw socket calls or locally duplicate protocol DTOs.
- Vocabulary is fixed: workspace, tab, pane, agent, repository, checkout, invocation, entrypoint, subscription, and metadata token.
- Use `InvocationKind::{Action, PaneEntrypoint, Startup, Event}` for exhaustive process dispatch; link-handler identity remains orthogonal.
- Read manifest identity from Herdr's environment exactly once; never duplicate action or entrypoint IDs in command arguments.
- Reject missing, conflicting, empty, out-of-range, or unknown boundary data before performing side effects.
- Preserve operation semantics in transport deadlines: short ordinary calls, server timeout plus slack, and no read deadline for indefinite waits.
- Keep response and event frames bounded, newline framed, request-ID checked, and result-tag checked.
- Use `Snapshot::effective_workspace_dir` for checkout-or-pane directory resolution; do not recreate fallback policy in plugins.
- Use `Client::agent_prompt_and_wait` whenever prompt submission and settlement must be one atomic Herdr operation.
- Model domain states with enums and newtypes; raw external DTOs stay private at their integration boundary.
- Revalidate every mutable external invariant immediately before mutations; lock ownership alone does not make cached state current.
- Persist reservations before external side effects and make retry recovery explicit and identity-based.
- Give spawned commands a wall-clock deadline and always drain, kill, and reap them; only explicitly interactive watchers may be unbounded.
- Manifests invoke only `./bin/herdr-<plugin>` and use their exact `plugin-build-<plugin>` Make target.
- Background features use `herdrkit::service`: startup/event hooks ensure a pane-less service and exit. Require a server-scoped OS lock, readiness handshake, bounded control messages, launch backoff, and owner/disable checks. Recovery is event-driven, not Herdr supervision. Keep provider scheduling and durable state in the plugin.
- Build through Cargo and the root Makefile; stage binaries by same-directory atomic rename rather than helper build scripts.
- Run `make check`: formatting, strict all-target Clippy, workspace `cargo-nextest`, doctests, and warnings-as-errors rustdoc are one gate.
- Keep manifest-to-dispatch and build contracts executable in `contracts`; a manifest change requires updating its contract expectation.
- Use `herdrkit::testing::Server` and owned `Step::reply`/`Step::bytes` fixtures; never leak allocations to manufacture static test data.
- Render complete error chains at binary boundaries and add context at each meaningful operation boundary.
- Never capture live machine, employer, or session data in fixtures; hand-author neutral protocol-realistic inputs.
- Never run `herdr server reload-config`, link plugins, or install plugins while editing; report that the user must perform live activation.
- A missing restow can surface as exit 127 in `herdr plugin log list`; run the root `make` before diagnosing a new PATH script.
- Follow `crates/herdrkit/src/api.rs` for shared API design and each plugin's `src/main.rs` for dispatch structure.

## Local review ownership

- Vocabulary: review context → thread → message; agent request → dispatch attempt. Do not use “conversation” as an identity.
- Review is ongoing local collaboration; neither an agent returning nor a PR state resolves a thread.
- `review-core` owns reservations, attempt authorization, recovery eligibility, and bounded request projections; adapters must not recreate those policies.
- Validate complete new messages and finding evidence before committing reservations; bound historical context without silently truncating new instructions.
- Freeze request inputs, overlay saved answers on retry, and never replay an answered thread as new work.
- Accept replies transactionally against the current expected attempt and observed caller; preserve idempotency and typed conflicts.
- Keep delivery outcome and saved-answer progress distinct; recovery is per request, never newest-request-only.
- Keep binding reads paginated and revision-consistent; merge fragments on every page and restart stale reads without repeating mutations.
- Neovim owns one coalesced watcher per attached binding, independent of dispatch; unchanged revisions must avoid full reloads.
- Keep code changes and GitHub publishing behind explicit user instructions, not implied by a review question.
- Follow `crates/review-core/src/store/reads.rs` for bounded reads and `review/ARCHITECTURE.md` for cross-layer contracts.
