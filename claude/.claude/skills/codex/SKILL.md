---
name: codex
description: "Cross-model second opinion from codex (GPT-5.x) via the codex MCP server. Reach for it before committing an expensive diff, on hard architecture or migration calls, after repeated failed attempts at the same bug, or to review a plan — a different vendor's model is the only true independent reviewer; Claude subagents share Claude's blind spots. Not for routine edits or anything faster to do directly."
---

# codex

Codex is a **second opinion**, not an authority. You are the primary agent; codex is a different-model consultant you can talk to. Its value is adversarial diversity: every Claude subagent shares Claude's blind spots, so when you want truly independent eyes — not just more of the same kind — this is the tool.

Treat its output as a claim, not a verdict. Check every claim against the actual code before repeating it — codex may reference symbols, files, or behavior that don't exist. Compiler and test output outrank anything codex asserts. When you relay a codex point, say it came from codex and say what you verified. Form your own view before presenting; don't launder its answer through as your own conclusion.

Back-and-forth is encouraged. Debate, push back, ask it to justify claims, or challenge its suggestions with context it may not have. A two- or three-turn conversation that stress-tests an idea beats a single unchallenged answer.

## When to delegate

- **Code / diff review** — where a missed bug is expensive.
- **Hard architecture or migration problems** — genuinely stuck, or a call worth validating with a dissenting design.
- **Second opinion on a plan** — before committing to an approach.
- **A bug you've failed at twice** — a fresh model may see it.
- **Exploring an unfamiliar codebase** — "figure out how X works and report back."

Do not delegate routine edits, small refactors, or anything faster to do directly. Cross-model review costs latency and money; spend it where independence pays.

## Calling convention

Two tools: `mcp__codex__codex` starts a thread, `mcp__codex__codex-reply` continues one. Never shell out to `codex exec` — the MCP path returns a structured `{threadId, content}` and needs no output files, no JSONL parsing, and no re-asserting of settings on follow-ups.

```
mcp__codex__codex
  prompt:           "Review the diff on this branch for correctness bugs."
  sandbox:          "read-only"
  cwd:              "/abs/path/to/repo"
  approval-policy:  "never"
  config:           {"model_reasoning_effort": "high"}
```

- `sandbox: "read-only"` is load-bearing and must be passed explicitly: the config default is `workspace-write`, and trusted projects get it.
- `approval-policy: "never"` keeps a consult backgroundable. Under the config default (`on-request`) codex can raise an approval request, and a call waiting on an open elicitation dialog never moves to a background task — it pins the foreground instead. Denials come back as ordinary command errors, which is what you want: a degraded answer beats a stalled turn.
- `cwd` should be an absolute path to the repo root. Relative paths resolve against the *server's* cwd, not yours.
- Never set `model` — the configured default is deliberately the strongest available, so any override can only downgrade (and hardcoded model strings rot on update cycles).
- Always pass `model_reasoning_effort` explicitly. Omitting it inherits whatever `~/.codex/config.toml` currently says, which is tuned for interactive use, unstowed, and therefore different per machine — a consult's depth should not be a side effect of an unrelated setting.

## Choosing reasoning effort

Valid values: `none, minimal, low, medium, high, xhigh, max`. There is no `auto`.

**`high` is the floor.** You only reach for codex on decisions expensive enough to justify cross-model review; anything cheap enough for `medium` is cheap enough to not consult at all.

Escalate to `xhigh` or `max` on the conjunction of three things, not on task category — a two-line diff and a forty-file concurrency refactor are both "review":

- **Search, not evaluation.** Finding a defect nobody has pointed at rewards depth. Critiquing a plan already laid out in the prompt mostly does not.
- **Simultaneity.** Many interacting parts that must be held at once — lock ordering, migration sequencing, cross-module invariants — rather than localized reasoning.
- **Unverifiable output.** If a compiler or a test can settle it, depth is the wrong lever; get a fast answer and check it. Depth earns its cost when the answer is a judgment call you cannot execute.

**Prefer another round over a higher dial.** Two turns at `high` beat one at `max` and are strictly more useful, because you steer the second turn with what the first one got wrong. Raise effort only when a follow-up cannot help: a single-shot judgment call, or a search you have already sharpened the prompt for and it still came back generic. A vague prompt at `max` loses to a precise one at `high`.

**Effort and sandbox are locked at thread start.** `codex-reply` takes only `threadId` and `prompt`, so you cannot raise the dial mid-conversation — escalating means abandoning the thread and starting a fresh one at the higher level, losing the accumulated context. When a consult looks like it may get harder as it goes, open it one notch above what the first question needs.

There is no diff-target flag on this path. Say what to review in the prompt and let codex run git itself — "review the changes on this branch against main", "review the uncommitted changes including untracked files". Read-only sandbox permits the git commands it needs.

Sandbox denials don't abort the run — codex absorbs the error and finishes with a degraded answer. If a response looks like it couldn't read or run something, that is usually why, and the fix is a better-scoped prompt rather than more permission. Escalating to `sandbox: "workspace-write"` is a real permission decision, not a prompt detail: tell the user before doing it.

## Run lifecycle

A consult is launch → work on something else → notification → read the answer.

1. **Launch.** A call still running past the backgrounding threshold (`CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS`, two minutes by default) moves to a background task automatically; you get a task id immediately and keep working. Real consults routinely exceed it, so expect backgrounding rather than an inline answer.
2. **Wait by working.** The result arrives as a task notification. Never sit in `sleep`/poll loops, and never re-check "is it done yet" between unrelated steps. If nothing else is actionable until codex answers, end the turn and pick up when the notification lands.
3. **On completion, the answer is the tool result** — `content` is the final message, `threadId` is what you resume with. Nothing to read off disk.
4. **Kill runs you no longer need** (TaskStop). A running consult burns real tokens and minutes; the moment its question is answered by other evidence, the user changes direction, or it was speculative to begin with, stop it. Don't leave codex running as a background hedge. Killing mid-turn abandons that turn's work — the thread's completed turns stay resumable.

One consult at a time unless the questions are genuinely independent — parallel codex runs on the same topic can't see each other and just duplicate spend.

Backgrounding cannot be requested. Every call starts in the main conversation and only detaches once the threshold expires, so a short consult answers inline and a long one costs that much foreground first. There is no flag or argument to skip the wait — only the threshold itself, which is global.

## Backgrounding a review

**Run reviews from the main thread and let them detach.** One call, the raw answer in your context, the `threadId` already in hand for follow-ups. A long review costs a short foreground wait and then notifies you; that is the whole workflow, and it needs no scaffolding.

Delegating the consult to a subagent is tempting and usually wrong. It looks like it buys verification for free — hand the worker the consult *and* the job of checking each claim against the code, get back a confirmed shortlist. What it actually buys is a Claude instance deciding which of codex's findings deserve your attention, and this skill exists because Claude subagents share Claude's blind spots. A finding that sits outside Claude's priors is exactly the finding worth paying for, and exactly the one a Claude filter drops. The layer also compresses: what returns is the subagent's account of codex, not codex, and you cannot audit what it discarded without reading a transcript too large to read.

**Delegate only under context pressure** — a review whose raw output would genuinely crowd out the work in progress. That is a resource trade, not a quality one, and it costs fidelity. When you make it, give the worker the whole contract, because a subagent does not inherit the tool schema and cannot call codex at all without loading it first:

> Consult codex on <scope> via `mcp__codex__codex` (read-only, `approval-policy: never`, effort `high`, `cwd` <repo root>). Load the tool with `ToolSearch` first — `select:mcp__codex__codex`, or the call fails with `InputValidationError`. Return the `threadId`, codex's findings with file and line, and your own note on any you could not substantiate — report all of them, do not filter.

Have it report everything and mark its doubts rather than pre-filtering, so the blind-spot loss is bounded to ordering rather than deletion. Threads belong to the shared MCP server process, so the `threadId` it returns is resumable with `codex-reply` from the main thread — take the conversation back as soon as the bulk output is behind you.

## Continuing a thread

```
mcp__codex__codex-reply
  threadId: "01a0690b-4730-7b52-bc4d-30a47ba57fd2"
  prompt:   "Your KeyRotator race claim — walk me through the interleaving. mu is held across both loads."
```

The thread keeps the sandbox and effort it was created with; `codex-reply` takes only `threadId` and `prompt`, and there is nothing to re-assert. Threads stay live for the whole session and survive arbitrary intervening work, so a consult you launched and walked away from is still there when you come back.

Pass the `threadId` from the tool result verbatim. There is no "most recent thread" shortcut, and both ways of getting it wrong fail loudly rather than answering without context — an unknown id gives `Session not found for thread_id`, and omitting it gives `either threadId or conversationId must be provided`. Ignore `conversationId`; it is the deprecated spelling.

## Thread reuse

Before each call, state your intent in one line:

- *"Starting new codex session — reviewing auth.go."*
- *"Resuming thread 019f…bda3 — pushing back on the middleware suggestion."*

**Default: new thread per invocation.**

**Reply when:**
- Directly iterating on what codex just said (refining, pushing back, applying its suggestion to adjacent code).
- A follow-up where codex's prior context makes the answer better.

**Start fresh when:**
- The topic changed.
- You moved to different code or a different question.
- Codex's last answer was off-track.
- Many unrelated tool calls have happened since the last codex exchange.

When in doubt: new thread. Cheaper to restart than to poison an ongoing one.

## Push past the first answer

Codex's first response is usually a survey — competent but surface-level. The real value comes from follow-ups. After the initial response, always consider a reply that pushes for specifics: edge cases codex missed, things it hedged on, or applying its general suggestion to the concrete code at hand. Two or three focused replies on the same thread almost always converge on a better answer than a single elaborate prompt.

If codex's first answer is already sharp and specific — and you've verified it — a follow-up isn't needed. But when the response reads like a checklist or stays generic, push back.

## Prompt shape

Codex responds to operator-style prompts, not collaborative ones. One task per thread; split unrelated asks. Tell it what done looks like rather than assuming it infers the end state.

For review and research, demand grounding explicitly: every claim anchored to a file and line, hypotheses labelled as hypotheses. That is what makes its output cheap for you to verify, and unverifiable review output is worthless here.

Asking for a fixed output shape pays off when you intend to work through the findings — severity, file, line range, confidence, and recommendation per finding, ordered by severity. State the shape in the prompt.

## Auth

Login state lives in `~/.codex/auth.json` and expires on its own schedule. `codex login status` (non-interactive) confirms it; `codex doctor` covers install, config, auth, and runtime health together. Expired auth does not fail clean — expect ~30s of reconnect churn ending in a bare `401 Unauthorized`. Don't retry through it; surface it to the user to rerun `codex login`.

## Additional directories

When this Claude session has additional directories loaded (via `/add-dir` or similar), codex needs to be told about them — check what's loaded before delegating and name every relevant path. Access is rarely the issue (the read-only sandbox reads broadly); awareness is:

```
Review this change. The main project is at /path/to/repo, but it depends on
a shared library at /path/to/shared-lib — check for breaking interface changes
across both.
```

Write access outside `cwd` is not expressible on this path. If a task genuinely needs codex writing to a second repo, that's a `codex exec -s workspace-write --add-dir` run — raise it with the user rather than improvising.

## Setup

The server is registered at user scope in `~/.claude.json`, which is machine-local and not stowed; `make mcp` in the dots repo registers it idempotently. If `mcp__codex__*` tools aren't available, that's the first thing to check — an `mcpServers` block in `settings.json` is silently ignored by Claude Code.
