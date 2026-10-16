import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import type { ExtensionAPI, ExtensionContext, ExtensionToolContext } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { CHILD_CONTRACT, loadConfig, resolveModel } from "./config.ts";
import { PiProcess, type RpcRecord } from "./rpc.ts";
import { transcriptTail } from "./tail.ts";

// Workers: background Pi children with their own context and session. Five
// primitives: spawn, message, wait, status, stop. `message` is the only
// communication primitive and is not directional: any session sends to any
// address it can see (`parent`, or a worker it owns, or a sibling by id via
// the owner). Delivery adapts to the target's state: answered if it is blocked
// waiting, steered if running, continued in its own session if finished.
// Return is the event: a worker's settle, question, or message arrives as a
// custom message that starts the owner's next turn.

type State = "running" | "asking" | "completed" | "failed" | "stopped" | "paused";
type EventKind = State | "update" | "message" | "undeliverable";

interface Worker {
  id: string;
  label: string;
  task: string;
  cwd: string;
  model?: string;
  thinking?: string;
  tools?: string[];
  system?: string;
  sessionDir: string;
  sessionFile?: string;
  state: State;
  startedAt: number;
  updatedAt: number;
  tool?: string;
  tokens: number;
  question?: { uiId: string; text: string; method?: string };
  result?: string;
  error?: string;
  process?: PiProcess;
  /** True from our prompt/message until the first settle answers it; later
   * spontaneous settles (the child's own children waking it) are updates. */
  awaiting?: boolean;
  /** Bumped by every deliver/stop so an in-flight settle can tell it is stale. */
  generation?: number;
}

const ENTRY = "dots.worker";
/** Full result kept on the worker and in its session file. */
const MAX_RESULT = 12_000;
/** What an event carries into the owner's context; the rest is one status call away. */
const EVENT_CHARS = 1_500;
const EVENT_LINES = 12;

/** Bound a result for the event body without losing its lead: outcome first. */
function excerpt(text: string): { body: string; truncated: boolean } {
  const lines = text.split("\n");
  let body = lines.slice(0, EVENT_LINES).join("\n");
  if (body.length > EVENT_CHARS) body = body.slice(0, EVENT_CHARS);
  const truncated = body.length < text.length;
  return { body: truncated ? `${body.trimEnd()}\n…` : body, truncated };
}
/** Prefix for messages a child sends upward through the extension-UI notify channel. */
const MARK = "dots.message:";

function depth(env: NodeJS.ProcessEnv = process.env): number {
  const value = Number(env.DOTS_WORKER_DEPTH ?? 0);
  return Number.isFinite(value) && value >= 0 ? value : 0;
}

/** Set in worker sessions; `parent` is a valid address only there. */
function selfId(env: NodeJS.ProcessEnv = process.env): string | undefined {
  return env.DOTS_WORKER_ID || undefined;
}

export function sessionsRoot(env: NodeJS.ProcessEnv = process.env): string {
  return path.join(env.PI_CODING_AGENT_DIR ?? path.join(env.HOME ?? homedir(), ".pi/agent"), "dots/workers");
}

function label(text: string): string {
  return text.replace(/[\x00-\x1f\x7f-\x9f]/g, " ").trim().slice(0, 60);
}

function elapsed(since: number): string {
  const seconds = Math.max(0, Math.round((Date.now() - since) / 1000));
  return seconds < 60 ? `${seconds}s` : seconds < 3600 ? `${Math.round(seconds / 60)}m` : `${(seconds / 3600).toFixed(1)}h`;
}

/** Launch arguments shared by a new child and a continued one, so a worker
 * keeps its model, tools, and contract across reload. The nesting guard
 * removes only spawning. */
function launchArgs(worker: Pick<Worker, "label" | "model" | "thinking" | "tools" | "system">, childDepth: number, maxDepth: number): string[] {
  const args = [
    "--name", worker.label,
    "--append-system-prompt", [worker.system?.trim(), CHILD_CONTRACT].filter(Boolean).join("\n\n"),
  ];
  if (childDepth >= maxDepth) args.push("--exclude-tools", "worker_spawn");
  if (worker.model) args.push("--model", worker.model);
  if (worker.thinking) args.push("--thinking", worker.thinking);
  if (worker.tools?.length) args.push("--tools", [...new Set([...worker.tools, "message"])].join(","));
  return args;
}

/** How to answer each blocking dialog method, per Pi's extension-UI protocol. */
function uiAnswer(method: string | undefined, text: string): Record<string, unknown> {
  if (method === "confirm") return { confirmed: /^(y|yes|true|ok|confirm)/i.test(text.trim()) };
  return { value: text };
}

function persisted(worker: Worker) {
  const { process: _process, question, ...rest } = worker;
  return { ...rest, question: question?.text };
}

export default function (pi: ExtensionAPI) {
  const workers = new Map<string, Worker>();
  // Contexts are invalidated when the runtime is replaced (reload, resume), so
  // keep the most recent one from any event or tool call for widget updates.
  let context: ExtensionContext | undefined;
  const track = (ctx: ExtensionContext) => {
    context = ctx;
  };

  const save = (worker: Worker) => {
    worker.updatedAt = Date.now();
    pi.appendEntry(ENTRY, persisted(worker));
    widget();
  };

  const widget = () => {
    if (!context?.hasUI) return;
    const live = [...workers.values()].filter((w) => w.state === "running" || w.state === "asking");
    if (live.length === 0) {
      context.ui.setWidget("dots.workers", undefined);
      return;
    }
    const lines = live.map((w) => {
      const detail = w.state === "asking" ? `asking: ${w.question?.text ?? ""}` : w.tool ? `${w.tool}` : "thinking";
      return `${w.state === "asking" ? "?" : "●"} ${w.label} · ${elapsed(w.startedAt)} · ${w.tokens.toLocaleString("en-US")} tok · ${detail}`.slice(0, 160);
    });
    context.ui.setWidget("dots.workers", [`workers (${live.length})`, ...lines], { placement: "belowEditor" });
  };

  // Waiters: resolved whenever a worker settles, asks, messages, or dies, so a
  // session with nothing else to do can block in worker_wait or message{wait}.
  const waiters = new Set<(worker: Worker, kind: EventKind, body: string) => void>();
  const wake = (worker: Worker, kind: EventKind, body: string) => {
    for (const waiter of [...waiters]) waiter(worker, kind, body);
  };

  const notify = (worker: Worker, kind: EventKind, full: string, triggerTurn = true) => {
    wake(worker, kind, full);
    const { body, truncated } = excerpt(full);
    const more = truncated ? `\n\n[${full.length.toLocaleString("en-US")} chars; full text: worker_status { id: "${worker.id}" }]` : "";
    const header = `Worker ${worker.label} [${worker.id}] ${kind}${worker.sessionFile ? ` · session ${worker.sessionFile}` : ""}`;
    // Idle owner: starts a turn. Busy owner: steered into the current run at
    // the next tool boundary, so a question or result is seen while it is
    // fresh instead of becoming a stale follow-up turn later. ("nextTurn"
    // would only park it until the user's next prompt.)
    pi.sendMessage(
      { customType: "dots.worker.event", content: `${header}\n\n${body}${more}`, display: true, details: { id: worker.id, kind, label: worker.label, truncated } },
      { triggerTurn, deliverAs: "steer" },
    );
  };

  const find = (id: string): Worker => {
    const wanted = id.trim();
    if (!wanted) throw new Error("a worker id or label is required");
    const exact = workers.get(wanted);
    if (exact) return exact;
    const matches = [...workers.values()].filter((w) => w.id.startsWith(wanted) || w.label === wanted);
    if (matches.length === 1) return matches[0]!;
    if (matches.length > 1) throw new Error(`${wanted} is ambiguous: ${matches.map((w) => `${w.id} (${w.label})`).join(", ")}`);
    throw new Error(`no worker ${wanted}; known: ${[...workers.keys()].join(", ") || "none"}`);
  };

  const attach = (worker: Worker, child: PiProcess) => {
    worker.process = child;
    child.onRecord((record) => {
      if (worker.process === child) onRecord(worker, record);
    });
    child.onExit(() => {
      if (worker.process !== child) return; // a newer process owns this worker
      worker.process = undefined;
      if (worker.state === "running" || worker.state === "asking") {
        worker.state = "failed";
        worker.error = `pi exited unexpectedly; last stderr: ${child.stderr.slice(-3).join("").trim().slice(-500)}`;
        save(worker);
        notify(worker, "failed", worker.error);
      }
    });
  };

  const onRecord = (worker: Worker, record: RpcRecord) => {
    switch (record.type) {
      case "tool_execution_start":
        worker.tool = String(record.toolName ?? "");
        worker.updatedAt = Date.now();
        widget();
        return;
      case "tool_execution_end":
        worker.tool = undefined;
        widget();
        return;
      case "message_update": {
        const usage = record.usage as { totalTokens?: number } | undefined;
        if (usage?.totalTokens) worker.tokens = usage.totalTokens;
        return;
      }
      case "agent_settled":
        settle(worker, record.aborted === true).catch((error) => {
          worker.error = `settle failed: ${(error as Error).message}`;
          save(worker);
        });
        return;
      case "extension_ui_request":
        onUiRequest(worker, record);
        return;
      default:
        return;
    }
  };

  /** The child's extension-UI channel carries two things we care about: a
   * blocking question (dialog methods) and our own upward messages (notify). */
  const onUiRequest = (worker: Worker, record: RpcRecord) => {
    const method = String(record.method ?? "");
    if (method === "input" || method === "editor" || method === "select" || method === "confirm") {
      worker.state = "asking";
      worker.question = { uiId: String(record.id ?? ""), text: label(String(record.title ?? record.message ?? "question")), method };
      save(worker);
      notify(worker, "asking", `${String(record.title ?? "")}\n\nAnswer with message { to: "${worker.id}", text }.`);
      return;
    }
    if (method !== "notify") return;
    const text = String(record.message ?? "");
    if (!text.startsWith(MARK)) return;
    let envelope: { to?: string; text?: string };
    try {
      envelope = JSON.parse(text.slice(MARK.length));
    } catch {
      return;
    }
    const body = envelope.text ?? "";
    if (!envelope.to || envelope.to === "parent") {
      notify(worker, "message", body);
      return;
    }
    // Sibling-addressed: route through this registry, visibly.
    let target: Worker;
    try {
      target = find(envelope.to);
    } catch (error) {
      notify(worker, "undeliverable", `${(error as Error).message}\n\nOriginal message:\n${body}`);
      return;
    }
    pi.sendMessage(
      { customType: "dots.worker.event", content: `Worker ${worker.label} [${worker.id}] → ${target.label} [${target.id}]\n\n${body}`, display: true, details: { id: worker.id, kind: "relay", to: target.id } },
      { triggerTurn: false },
    );
    deliver(target, `Message from worker ${worker.label} [${worker.id}]:\n\n${body}`).catch((error) => {
      notify(worker, "undeliverable", `${(error as Error).message}\n\nOriginal message:\n${body}`);
    });
  };

  const settle = async (worker: Worker, aborted: boolean) => {
    if (!worker.process || worker.state === "stopped") return;
    const generation = worker.generation ?? 0;
    const process = worker.process;
    let text = "";
    let readError: string | undefined;
    try {
      const data = await process.call<{ text: string | null }>({ type: "get_last_assistant_text" });
      text = data.text ?? "";
      const state = await process.call<{ sessionFile?: string }>({ type: "get_state" });
      worker.sessionFile = state.sessionFile ?? worker.sessionFile;
    } catch (error) {
      readError = (error as Error).message;
    }
    // A deliver or stop that happened while we were reading owns the worker now.
    if ((worker.generation ?? 0) !== generation || worker.process !== process) return;
    const bounded = text.length > MAX_RESULT ? `${text.slice(0, MAX_RESULT)}\n\n[truncated; full transcript in the session file]` : text;
    worker.tool = undefined;
    if (readError) {
      worker.state = "failed";
      worker.error = `could not read the result: ${readError}`;
      worker.awaiting = false;
      save(worker);
      notify(worker, "failed", worker.error);
      return;
    }
    if (!worker.awaiting && worker.state !== "running") {
      // A settle we did not ask for: the child's own events woke it. Report,
      // but keep the answer to our last message as the result.
      save(worker);
      notify(worker, "update", bounded || "(no text)");
      return;
    }
    worker.result = bounded;
    worker.state = aborted ? "stopped" : "completed";
    worker.awaiting = false;
    save(worker);
    // Keep the process so a follow-up can continue the same context cheaply;
    // close it only on stop or shutdown.
    notify(worker, worker.state, worker.result || "(no final text)");
  };

  const spawn = async (
    ctx: ExtensionContext,
    input: { task: string; label: string; cwd?: string; model?: string; tools?: string[]; system?: string },
  ): Promise<Worker> => {
    const config = loadConfig();
    const resolved = resolveModel(input.model, config);
    if (resolved.error) throw new Error(resolved.error);
    const cwd = path.resolve(ctx.cwd, input.cwd ?? ".");
    if (!existsSync(cwd)) throw new Error(`cwd does not exist: ${cwd}`);
    const childDepth = depth() + 1;
    if (childDepth > config.maxDepth) throw new Error(`worker nesting limit reached (maxDepth ${config.maxDepth} in workers.json)`);
    const id = `w-${randomBytes(3).toString("hex")}`;
    const sessionDir = path.join(sessionsRoot(), id);
    mkdirSync(sessionDir, { recursive: true });
    const worker: Worker = {
      id, label: label(input.label), task: input.task, cwd, model: resolved.model, thinking: resolved.thinking,
      tools: input.tools?.length ? input.tools : undefined, system: input.system?.trim() || undefined, sessionDir,
      state: "running", startedAt: Date.now(), updatedAt: Date.now(), tokens: 0, awaiting: true, generation: 0,
    };
    const args = ["--session-dir", sessionDir, ...launchArgs(worker, childDepth, config.maxDepth)];
    const child = new PiProcess({ cwd, args, env: { DOTS_WORKER_ID: id, DOTS_WORKER_DEPTH: String(childDepth) } });
    attach(worker, child);
    workers.set(id, worker);
    try {
      const disposition = await child.call<{ disposition: string }>({ type: "prompt", message: input.task });
      if (disposition.disposition === "handled") throw new Error("the child handled the prompt without starting a run");
    } catch (error) {
      // Never leave a ghost: record the failure and end the process.
      worker.process = undefined;
      worker.state = "failed";
      worker.error = `${(error as Error).message}${child.stderr.length ? `; stderr: ${child.stderr.join("").trim().slice(-500)}` : ""}`;
      save(worker);
      await child.close(1000);
      throw new Error(`${worker.id} failed to start: ${worker.error}`);
    }
    try {
      const state = await child.call<{ sessionFile?: string }>({ type: "get_state" });
      worker.sessionFile = state.sessionFile;
    } catch {
      // settle() reads it again
    }
    save(worker);
    return worker;
  };

  /** Deliver text to a worker we own, however it is currently occupied. */
  const deliver = async (worker: Worker, text: string): Promise<string> => {
    if (worker.state === "stopped" && worker.process) throw new Error(`${worker.id} is stopping`);
    worker.generation = (worker.generation ?? 0) + 1;
    if (worker.state === "asking" && worker.question && worker.process) {
      worker.process.respondUi(worker.question.uiId, uiAnswer(worker.question.method, text));
      worker.state = "running";
      worker.question = undefined;
      worker.awaiting = true;
      save(worker);
      return "answered its question";
    }
    if (worker.state === "running" && worker.process) {
      await worker.process.call({ type: "steer", message: text });
      return "steered";
    }
    if (!worker.process) {
      if (!worker.sessionFile) throw new Error(`${worker.id} has no session to continue`);
      const childDepth = depth() + 1;
      const child = new PiProcess({
        cwd: worker.cwd,
        args: ["--session", worker.sessionFile, ...launchArgs(worker, childDepth, loadConfig().maxDepth)],
        env: { DOTS_WORKER_ID: worker.id, DOTS_WORKER_DEPTH: String(childDepth) },
      });
      attach(worker, child);
    }
    worker.state = "running";
    worker.awaiting = true;
    worker.result = undefined;
    save(worker);
    await worker.process!.call({ type: "prompt", message: text });
    return "continued in its own session";
  };

  /** Block until a worker (or any) settles, asks, or messages; undefined on timeout/abort. */
  const waitFor = (target: Worker | undefined, timeoutMs: number, signal?: AbortSignal) =>
    new Promise<{ worker: Worker; kind: EventKind; body: string } | undefined>((resolve) => {
      const finish = (value: { worker: Worker; kind: EventKind; body: string } | undefined) => {
        clearTimeout(timer);
        waiters.delete(waiter);
        signal?.removeEventListener("abort", onAbort);
        resolve(value);
      };
      const timer = setTimeout(() => finish(undefined), timeoutMs);
      const waiter = (worker: Worker, kind: EventKind, body: string) => {
        if (!target || worker.id === target.id) finish({ worker, kind, body });
      };
      const onAbort = () => finish(undefined);
      waiters.add(waiter);
      if (signal?.aborted) {
        finish(undefined);
        return;
      }
      signal?.addEventListener("abort", onAbort);
    });

  /** Cancel a pending wait (used when delivery fails after the waiter was registered). */
  const cancelWait = (worker: Worker) => wake(worker, "update", "");

  const stop = async (worker: Worker): Promise<void> => {
    worker.generation = (worker.generation ?? 0) + 1;
    worker.state = "stopped";
    worker.question = undefined;
    const process = worker.process;
    worker.process = undefined; // no late exit or settle can touch this worker now
    save(worker);
    wake(worker, "stopped", "stopped");
    if (process) {
      try {
        await process.call({ type: "abort" });
      } catch {
        // already gone
      }
      await process.close(2000);
    }
  };

  const summary = (worker: Worker) => ({
    id: worker.id,
    label: worker.label,
    state: worker.state,
    elapsed: elapsed(worker.startedAt),
    tokens: worker.tokens,
    tool: worker.tool,
    question: worker.question?.text,
    cwd: worker.cwd,
    model: worker.model,
    session: worker.sessionFile,
    result: worker.result,
    error: worker.error,
  });

  pi.on("before_agent_start", (_event, ctx) => track(ctx));
  pi.on("agent_end", (_event, ctx) => track(ctx));

  pi.on("session_start", (_event, ctx) => {
    track(ctx);
    // Children died with the previous process; their sessions persist and can be continued.
    for (const entry of ctx.sessionManager.getBranch()) {
      if (entry.type !== "custom" || entry.customType !== ENTRY) continue;
      const data = entry.data as ReturnType<typeof persisted>;
      const existing = workers.get(data.id);
      if (existing?.process) continue;
      const state: State = data.state === "running" || data.state === "asking" ? "paused" : data.state;
      workers.set(data.id, { ...data, state, question: undefined, tokens: data.tokens ?? 0 });
    }
    widget();
  });

  pi.on("session_shutdown", async () => {
    const closing: Promise<void>[] = [];
    for (const worker of workers.values()) {
      const process = worker.process;
      if (!process) continue;
      worker.process = undefined;
      worker.state = worker.state === "running" || worker.state === "asking" ? "paused" : worker.state;
      pi.appendEntry(ENTRY, persisted(worker));
      closing.push(process.close(3000));
    }
    await Promise.all(closing);
  });

  pi.registerTool({
    name: "worker_spawn",
    label: "Spawn worker",
    description:
      "Start a background Pi child with its own context and session. Returns immediately with an id; its final message arrives later as a worker event that starts your next turn. Give it a self-contained brief: target (files, non-goals), change, acceptance, checks, stopping point. Use cwd for a worktree. model is a provider/model[:thinking] id or @name from workers.json modelRoles; omit to inherit. tools is an allowlist; omit for the defaults.",
    promptSnippet: "Delegate a bounded piece of work to a background child; results come back as events.",
    parameters: Type.Object({
      label: Type.String({ description: "Short task-named label, e.g. netpol-api" }),
      task: Type.String({ description: "The complete brief" }),
      cwd: Type.Optional(Type.String({ description: "Working directory, usually a worktree; relative to this session's cwd" })),
      model: Type.Optional(Type.String({ description: "provider/model[:thinking] or @role" })),
      tools: Type.Optional(Type.Array(Type.String(), { description: "Tool allowlist, e.g. [read, grep, find, ls] for read-only; message is always included" })),
      system: Type.Optional(Type.String({ description: "Extra system prompt for this child; the child contract is always appended" })),
    }),
    async execute(_id, params, _signal, _update, ctx) {
      track(ctx);
      const worker = await spawn(ctx, params);
      return { content: [{ type: "text", text: `Spawned ${worker.id} (${worker.label}) in ${worker.cwd}${worker.model ? ` on ${worker.model}` : ""}. Its result will arrive as a worker event; continue with other work.` }], details: summary(worker) };
    },
  });

  pi.registerTool({
    name: "message",
    label: "Message",
    description:
      "Send text to another session: a worker you spawned (by id or label), a sibling worker (by id; the owner relays it), or \"parent\" when you are a worker. Delivery adapts to the target: it answers a worker that is blocked on a question, steers a running one between tool calls, or continues a finished or paused one in its own session. wait: true blocks until the target replies or settles and returns that text; use it for questions that need an answer before you can proceed.",
    promptSnippet: "Talk to a worker or your parent; wait: true to block for the reply.",
    parameters: Type.Object({
      to: Type.String({ description: "Worker id or label, or \"parent\"" }),
      text: Type.String(),
      wait: Type.Optional(Type.Boolean({ description: "Block until the target replies or settles (default false)" })),
      timeoutMs: Type.Optional(Type.Number({ description: "With wait: give up after this long (default 600000)" })),
    }),
    async execute(_id, params, signal, _update, ctx) {
      track(ctx);
      const me = selfId();
      if (params.to === "parent") {
        if (!me) throw new Error("this session has no parent");
        if (params.wait) {
          const answer = await ctx.ui.input(params.text);
          return { content: [{ type: "text", text: answer === undefined ? "The parent did not answer. Proceed with your recommendation and say so in your result." : answer }], details: { answered: answer !== undefined } };
        }
        ctx.ui.notify(`${MARK}${JSON.stringify({ to: "parent", text: params.text })}`, "info");
        return { content: [{ type: "text", text: "sent to parent" }], details: { to: "parent" } };
      }
      let worker: Worker | undefined;
      try {
        worker = find(params.to);
      } catch (error) {
        if (!me) throw error;
        // Not ours: a sibling. The owner routes it through its registry.
        ctx.ui.notify(`${MARK}${JSON.stringify({ to: params.to, text: params.text })}`, "info");
        return { content: [{ type: "text", text: `sent to ${params.to} via the parent` }], details: { to: params.to, relayed: true } };
      }
      const pending = params.wait ? waitFor(worker, params.timeoutMs ?? 600_000, signal) : undefined;
      let disposition: string;
      try {
        disposition = await deliver(worker, params.text);
      } catch (error) {
        if (pending) cancelWait(worker);
        throw error;
      }
      if (!pending) {
        return { content: [{ type: "text", text: `${worker.id}: ${disposition}` }], details: summary(worker) };
      }
      const outcome = await pending;
      if (!outcome) return { content: [{ type: "text", text: `${worker.id}: ${disposition}; no reply within the wait window (still ${worker.state})` }], details: summary(worker) };
      return { content: [{ type: "text", text: `${worker.id} ${outcome.kind}\n\n${outcome.body}` }], details: summary(worker) };
    },
  });

  pi.registerTool({
    name: "worker_wait",
    label: "Wait for worker",
    description: "Block until a worker settles, asks a question, or sends a message; without id, the first worker to do so. Returns what happened. Use when you have nothing else to do; an interactive parent does not need this because events start its next turn.",
    parameters: Type.Object({
      id: Type.Optional(Type.String()),
      timeoutMs: Type.Optional(Type.Number({ description: "Give up after this long (default 600000); the worker keeps running" })),
    }),
    async execute(_id, params, signal, _update, ctx) {
      track(ctx);
      const target = params.id ? find(params.id) : undefined;
      const live = (w: Worker) => w.state === "running";
      const candidates = target ? [target] : [...workers.values()];
      const already = candidates.find((w) => !live(w));
      if (already && (target || candidates.every((w) => !live(w)))) {
        return { content: [{ type: "text", text: `${already.id} is ${already.state}${already.result ? `\n\n${already.result}` : ""}${already.question ? `\n\nasking: ${already.question.text}` : ""}` }], details: summary(already) };
      }
      const outcome = await waitFor(target, params.timeoutMs ?? 600_000, signal);
      if (!outcome) {
        const rows = candidates.filter(live).map((w) => `${w.id} ${w.state} ${w.label} · ${elapsed(w.startedAt)}${w.tool ? ` · ${w.tool}` : ""}`);
        return { content: [{ type: "text", text: `No change within the wait window; still running:\n${rows.join("\n")}` }], details: candidates.map(summary) };
      }
      return { content: [{ type: "text", text: `${outcome.worker.id} ${outcome.kind}\n\n${outcome.body}` }], details: summary(outcome.worker) };
    },
  });

  pi.registerTool({
    name: "worker_status",
    label: "Worker status",
    description: "List workers of this session, or inspect one: state, elapsed, tokens, current tool, question, session file, result, and a tail of its recent tool calls and messages. Completions arrive as events; use this to look inside, not to wait.",
    parameters: Type.Object({
      id: Type.Optional(Type.String()),
      tail: Type.Optional(Type.Number({ description: "Recent transcript lines to include for one worker (default 20)" })),
    }),
    async execute(_id, params, _signal, _update, ctx) {
      track(ctx);
      const rows = params.id ? [find(params.id)] : [...workers.values()];
      const text = rows.length === 0
        ? "No workers in this session."
        : rows.map((w) => {
          const preview = !params.id && w.result ? `\n  result: ${label(w.result.split("\n").find((line) => line.trim()) ?? "")}` : "";
          let out = `${w.id}  ${w.state.padEnd(9)} ${w.label} · ${elapsed(w.startedAt)} · ${w.tokens} tok${w.tool ? ` · ${w.tool}` : ""}${w.question ? ` · asking: ${w.question.text}` : ""}${preview}${w.sessionFile ? `\n  session: ${w.sessionFile}` : ""}${w.error ? `\n  error: ${w.error}` : ""}`;
          if (params.id) {
            const tail = transcriptTail(w.sessionFile, params.tail ?? 20);
            if (tail.length) out += `\n\nrecent:\n${tail.join("\n")}`;
            if (w.result) out += `\n\nresult:\n${w.result}`;
          }
          return out;
        }).join("\n");
      return { content: [{ type: "text", text }], details: rows.map(summary) };
    },
  });

  pi.registerTool({
    name: "worker_stop",
    label: "Stop worker",
    description: "Abort a worker and end its process. Its session file stays; message can continue it later.",
    parameters: Type.Object({ id: Type.String() }),
    async execute(_id, params, _signal, _update, ctx) {
      track(ctx);
      const worker = find(params.id);
      await stop(worker);
      return { content: [{ type: "text", text: `${worker.id} stopped` }], details: summary(worker) };
    },
  });

  pi.registerCommand("workers", {
    description: "List this session's workers; `/workers <id>` shows what one has been doing",
    handler: async (args, ctx) => {
      if (!ctx.hasUI) return;
      const wanted = args.trim();
      if (wanted) {
        const worker = find(wanted);
        const tail = transcriptTail(worker.sessionFile, 40);
        ctx.ui.notify(
          `${worker.id}  ${worker.state} ${worker.label} · ${elapsed(worker.startedAt)} · ${worker.tokens} tok${worker.tool ? ` · ${worker.tool}` : ""}\n${worker.sessionFile ? `pi --session ${worker.sessionFile}\n` : ""}\n${tail.join("\n") || "(no transcript yet)"}`,
          "info",
        );
        return;
      }
      const rows = [...workers.values()];
      ctx.ui.notify(
        rows.length === 0
          ? `No workers. Sessions are stored under ${sessionsRoot()}.`
          : rows.map((w) => `${w.id}  ${w.state.padEnd(9)} ${w.label} · ${elapsed(w.startedAt)}${w.tool ? ` · ${w.tool}` : ""}${w.sessionFile ? `\n  pi --session ${w.sessionFile}` : ""}`).join("\n"),
        "info",
      );
    },
  });
}

export type { ExtensionToolContext };
