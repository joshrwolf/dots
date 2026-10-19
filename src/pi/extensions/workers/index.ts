import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import type { ExtensionAPI, ExtensionContext, ExtensionToolContext } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { CHILD_CONTRACT, loadConfig, resolveModel } from "./config.ts";
import { STALL_MS, attentionOrder, compact, createWidget, describeActivity, describeText, fleetStatus, renderEvent, type EventDetails, type WidgetRow } from "./present.ts";
import { openRegistry, type Registry } from "./registry.ts";
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

interface Question {
  uiId: string;
  text: string;
  method?: string;
  deadline?: number;
}

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
  /** Current call, or the last thing said/done between calls (glance text). */
  activity?: string;
  failed?: string;
  calls: number;
  lastEventAt: number;
  tokens: number;
  question?: Question;
  questions?: Question[];
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
const TICK_MS = 5_000;
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

/** Root worker id of the tree this session belongs to; set by the spawner. */
function rootId(env: NodeJS.ProcessEnv = process.env): string | undefined {
  return env.DOTS_WORKER_ROOT || undefined;
}

export function registryFile(env: NodeJS.ProcessEnv = process.env): string {
  return path.join(env.PI_CODING_AGENT_DIR ?? path.join(env.HOME ?? homedir(), ".pi/agent"), "dots/workers.db");
}

export function sessionsRoot(env: NodeJS.ProcessEnv = process.env): string {
  return path.join(env.PI_CODING_AGENT_DIR ?? path.join(env.HOME ?? homedir(), ".pi/agent"), "dots/workers");
}

/** One line of untrusted text for a listing; marks its own truncation. */
function label(text: string, max = 60): string {
  const clean = text.replace(/[\x00-\x1f\x7f-\x9f]/g, " ").replace(/\s+/g, " ").trim();
  return clean.length > max ? `${clean.slice(0, max - 1)}\u2026` : clean;
}

function duration(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m${s % 60 ? ` ${s % 60}s` : ""}`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
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
  if (worker.tools) args.push("--tools", [...new Set([...worker.tools, "message"])].join(","));
  return args;
}

/** How to answer each blocking dialog method, per Pi's extension-UI protocol. */
function uiAnswer(method: string | undefined, text: string): Record<string, unknown> {
  if (method === "confirm") return { confirmed: /^(?:y|yes|true|ok|confirm)$/i.test(text.trim()) };
  return { value: text };
}

function persisted(worker: Worker) {
  const { process: _process, question, questions: _questions, ...rest } = worker;
  return { ...rest, question: question?.text };
}

export default function (pi: ExtensionAPI) {
  const workers = new Map<string, Worker>();
  // Serialize transitions per worker, never across the fleet. Stop bypasses
  // this queue and invalidates queued work so an unresponsive child is stoppable.
  const transitions = new WeakMap<Worker, Promise<unknown>>();
  const epochs = new WeakMap<Worker, number>();
  const stopping = new WeakMap<Worker, Promise<void>>();
  const inOrder = <T>(worker: Worker, operation: () => Promise<T>): Promise<T> => {
    const epoch = epochs.get(worker) ?? 0;
    if (stopping.has(worker)) return Promise.reject(new Error(`${worker.id} is stopping; retry after it stops`));
    const next = (transitions.get(worker) ?? Promise.resolve()).catch(() => {}).then(() => {
      if (shuttingDown || epoch !== (epochs.get(worker) ?? 0)) throw new Error(`${worker.id} operation cancelled by stop or shutdown`);
      return operation();
    });
    transitions.set(worker, next);
    const clean = () => { if (transitions.get(worker) === next) transitions.delete(worker); };
    void next.then(clean, clean);
    return next;
  };
  // Contexts are invalidated when the runtime is replaced (reload, resume), so
  // keep the most recent one from any event or tool call for widget updates.
  let context: ExtensionContext | undefined;
  let shuttingDown = false;
  let maxWidgetRows = 8;
  let parentQuestionPending = false;
  const track = (ctx: ExtensionContext) => {
    if (!shuttingDown) context = ctx;
    for (const worker of workers.values()) refreshQuestions(worker);
  };

  // Observation plane: best-effort, never authoritative. Opened lazily so a
  // missing sqlite only disables the fleet view.
  let registry: Registry | undefined;
  let registryReady: Promise<void> | undefined;
  const ensureRegistry = () => registryReady ??= openRegistry(registryFile(), (message) => context?.ui.notify(message, "warning")).then((r) => { registry = r; });
  const withRegistry = <T>(read: (registry: Registry) => T): T | undefined => {
    if (!registry) return undefined;
    try { return read(registry); }
    catch (error) {
      const failed = registry;
      registry = undefined;
      try { failed.close(); } catch { /* already unavailable */ }
      if (!shuttingDown) context?.ui.notify(`worker registry disabled: ${(error as Error).message}`, "warning");
      return undefined;
    }
  };
  const observe = (worker: Worker) => {
    withRegistry((r) => r.upsert({
      id: worker.id, parent: selfId() ?? null, root: rootId() ?? worker.id,
      owner_session: context?.sessionManager.getSessionFile?.() ?? null,
      label: worker.label, state: worker.state, cwd: worker.cwd, model: worker.model ?? null,
      activity: worker.activity ?? null, failed: worker.failed ?? null, question: worker.question?.text ?? null,
      calls: worker.calls, tokens: worker.tokens, started_at: worker.startedAt, last_event_at: worker.lastEventAt,
      updated_at: Date.now(), session_file: worker.sessionFile ?? null, pid: process.pid,
    }));
  };
  const log = (worker: Worker, kind: "call" | "result" | "said" | "state" | "note", text: string) => {
    withRegistry((r) => r.event({ worker: worker.id, at: Date.now(), kind, text }));
  };
  const subtree = (worker: Worker) => withRegistry((r) => r.subtree(worker.id, Date.now(), STALL_MS));

  const save = (worker: Worker) => {
    if (shuttingDown) return;
    worker.updatedAt = Date.now();
    pi.appendEntry(ENTRY, persisted(worker));
    log(worker, "state", worker.state + (worker.error ? `: ${worker.error.slice(0, 200)}` : ""));
    observe(worker);
    widget();
  };

  // Widget rows: workers this session owns, each followed by its live
  // descendants from the registry (other runtimes' children), depth-first.
  const descendantRows = (parent: string, depth: number, now: number, seen = new Set<string>()): WidgetRow[] => {
    if (seen.has(parent)) return [];
    seen.add(parent);
    const rows = (withRegistry((r) => r.children(parent)) ?? []).map((r): WidgetRow & { id: string } => ({
      id: r.id, label: r.label, state: r.state, depth, elapsed: elapsed(r.started_at), tokens: r.tokens, calls: r.calls,
      detail: r.state === "asking" ? `asking: ${r.question ?? ""}` : (r.activity ?? r.state),
      failed: r.failed ?? undefined, idleMs: now - r.last_event_at,
    })).sort(attentionOrder);
    return rows.flatMap((row) => {
      if (seen.has(row.id)) return [];
      const children = descendantRows(row.id, depth + 1, now, seen);
      return row.state === "running" || row.state === "asking" || children.length ? [row, ...children] : [];
    });
  };
  const liveRows = (): WidgetRow[] => {
    for (const worker of workers.values()) refreshQuestions(worker);
    const now = Date.now();
    const own = [...workers.values()]
      .map((w): WidgetRow & { id: string } => ({
        id: w.id, label: w.label, state: w.state, depth: 0, elapsed: elapsed(w.startedAt), tokens: w.tokens, calls: w.calls,
        detail: w.state === "asking" ? `asking: ${w.question?.text ?? ""}` : (w.activity ?? "starting"),
        failed: w.failed, idleMs: now - w.lastEventAt,
      }))
      .sort(attentionOrder);
    return own.flatMap((row) => {
      const children = descendantRows(row.id, 1, now);
      if (row.state !== "running" && row.state !== "asking") {
        row.detail = `${row.state} · descendants active`;
        return children.length ? [row, ...children] : [];
      }
      return [row, ...children];
    });
  };

  // Elapsed and quiet times move without child events; tick while anything is live.
  let ticker: ReturnType<typeof setInterval> | undefined;
  const tick = (live: boolean) => {
    // Descendants change in other processes; the tick refreshes the fleet status too.
    if (live && !ticker) ticker = setInterval(widget, TICK_MS);
    if (!live && ticker) { clearInterval(ticker); ticker = undefined; }
  };

  // The widget is mounted once per live period and reads current rows on each
  // render; the host owns its disposal. Footer status carries the count.
  let widgetMounted = false;
  let widgetRows: WidgetRow[] = [];
  let requestRender: (() => void) | undefined;
  const widget = () => {
    if (shuttingDown || context?.mode !== "tui") return;
    const live = liveRows();
    tick(live.length > 0);
    widgetRows = live;
    context.ui.setStatus("workers", fleetStatus(live));
    if (live.length === 0) {
      if (widgetMounted) context.ui.setWidget("dots.workers", undefined);
      widgetMounted = false;
      requestRender = undefined;
      return;
    }
    if (!widgetMounted) {
      context.ui.setWidget("dots.workers", (tui, theme) => {
        requestRender = () => tui.requestRender();
        return createWidget(() => widgetRows, theme, maxWidgetRows);
      }, { placement: "belowEditor" });
      widgetMounted = true;
    } else {
      requestRender?.();
    }
  };
  // Waiters: resolved whenever a worker settles, asks, messages, or dies, so a
  // session with nothing else to do can block in worker_wait or message{wait}.
  const waiters = new Set<(worker: Worker, kind: EventKind, body: string) => boolean>();
  const cancelWaiters = new Set<() => void>();
  /** Returns true when some waiter took the outcome: the blocked caller already has the text. */
  const wake = (worker: Worker, kind: EventKind, body: string): boolean => {
    let consumed = false;
    for (const waiter of [...waiters]) consumed = waiter(worker, kind, body) || consumed;
    return consumed;
  };

  const notify = (worker: Worker, kind: EventKind, full: string, triggerTurn = true) => {
    if (shuttingDown) return;
    const consumed = wake(worker, kind, full);
    const { body, truncated } = excerpt(full);
    const more = truncated ? `\n\n[${full.length.toLocaleString("en-US")} chars; full text: worker_status { id: "${worker.id}" }]` : "";
    const header = `Worker ${worker.label} [${worker.id}] ${kind}${worker.sessionFile ? ` · session ${worker.sessionFile}` : ""}`;
    const details: EventDetails = { id: worker.id, kind, label: worker.label, truncated: truncated && !consumed, elapsed: elapsed(worker.startedAt), tokens: worker.tokens, session: worker.sessionFile };
    // Idle owner: starts a turn. Busy owner: steered into the current run at
    // the next tool boundary, so a question or result is seen while it is
    // fresh instead of becoming a stale follow-up turn later. ("nextTurn"
    // would only park it until the user's next prompt.)
    // A waiter already returned the full text to the model, so the event then
    // keeps only its first line: the transcript card stays, the context cost
    // does not double, and no extra turn starts.
    const first = body.split("\n").find((line) => line.trim()) ?? "";
    pi.sendMessage(
      { customType: "dots.worker.event", content: consumed ? `${header}\n\n${first}\n\n[delivered via worker_wait]` : `${header}\n\n${body}${more}`, display: true, details },
      { triggerTurn: triggerTurn && !consumed, deliverAs: "steer" },
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

  const refreshQuestions = (worker: Worker) => {
    worker.questions = (worker.questions ?? []).filter((question) => question.deadline === undefined || question.deadline > Date.now());
    worker.question = worker.questions[0];
    if (worker.state === "asking" && !worker.question) worker.state = "running";
    return worker.question;
  };

  const attach = (worker: Worker, child: PiProcess) => {
    worker.question = undefined;
    worker.questions = [];
    worker.process = child;
    child.onRecord((record) => {
      if (worker.process === child) onRecord(worker, record);
    });
    child.onExit(() => {
      if (worker.process !== child) return; // a newer process owns this worker
      worker.process = undefined;
      worker.question = undefined;
      worker.questions = [];
      worker.awaiting = false;
      if (worker.state === "running" || worker.state === "asking") {
        worker.state = "failed";
        worker.error = `pi exited unexpectedly; last stderr: ${child.stderr.slice(-3).join("").trim().slice(-500)}`;
        save(worker);
        notify(worker, "failed", worker.error);
      }
    });
  };

  const onRecord = (worker: Worker, record: RpcRecord) => {
    refreshQuestions(worker);
    switch (record.type) {
      case "agent_start":
        worker.generation = (worker.generation ?? 0) + 1;
        worker.state = "running";
        worker.error = undefined;
        worker.question = undefined;
        worker.questions = [];
        worker.lastEventAt = Date.now();
        save(worker);
        return;
      case "tool_execution_start":
        worker.tool = String(record.toolName ?? "");
        worker.activity = describeActivity(worker.tool, record.args);
        worker.calls += 1;
        worker.updatedAt = worker.lastEventAt = Date.now();
        log(worker, "call", worker.activity);
        observe(worker);
        widget();
        return;
      case "tool_execution_end": {
        const name = String(record.toolName ?? "");
        worker.tool = undefined;
        worker.failed = record.isError === true ? name : undefined;
        worker.lastEventAt = Date.now();
        if (record.isError === true) log(worker, "result", `${name} failed`);
        observe(worker);
        widget();
        return;
      }
      case "message_update": {
        const usage = record.usage as { totalTokens?: number } | undefined;
        if (usage?.totalTokens) worker.tokens = usage.totalTokens;
        worker.lastEventAt = Date.now();
        return;
      }
      case "message_end": {
        const message = record.message as { role?: string; stopReason?: string; errorMessage?: string; usage?: { totalTokens?: number } } | undefined;
        if (message?.role === "assistant") {
          worker.error = message.stopReason === "error" ? (message.errorMessage || "provider failed without an error message") : undefined;
          if (typeof message.usage?.totalTokens === "number") worker.tokens = message.usage.totalTokens;
        }
        const said = describeText(record.message);
        if (said) { worker.activity = `\u201c${said}\u201d`; log(worker, "said", said); observe(worker); widget(); }
        worker.lastEventAt = Date.now();
        return;
      }
      case "agent_settled": {
        worker.questions = [];
        worker.question = undefined;
        const generation = worker.generation ?? 0;
        const child = worker.process;
        void inOrder(worker, async () => {
          if (worker.process === child && (worker.generation ?? 0) === generation) await settle(worker, record.aborted === true);
        }).catch((error) => {
          if (shuttingDown || worker.process !== child || (worker.generation ?? 0) !== generation) return;
          worker.state = "failed";
          worker.error = `settle failed: ${(error as Error).message}`;
          save(worker);
          notify(worker, "failed", worker.error);
        });
        return;
      }
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
      const text = [record.title, record.message, Array.isArray(record.options) ? record.options.join(" | ") : undefined].filter((part) => typeof part === "string" && part).join("\n");
      const question: Question = { uiId: String(record.id ?? ""), text: text.slice(0, 4000) || "question", method };
      if (typeof record.timeout === "number" && Number.isFinite(record.timeout)) question.deadline = Date.now() + Math.max(0, record.timeout);
      (worker.questions ??= []).push(question);
      worker.state = "asking";
      refreshQuestions(worker);
      save(worker);
      if (worker.question) notify(worker, "asking", `${worker.question.text}\n\nAnswer the oldest pending question with message { to: "${worker.id}", text }.${worker.questions!.length > 1 ? ` ${worker.questions!.length} questions are pending.` : ""}`);
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
    if (!envelope || typeof envelope !== "object" || typeof envelope.text !== "string" || (envelope.to !== undefined && typeof envelope.to !== "string")) return;
    const body = envelope.text;
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
    let sessionFile: string | undefined;
    try {
      const data = await process.call<{ text: string | null }>({ type: "get_last_assistant_text" });
      text = data.text ?? "";
      const state = await process.call<{ sessionFile?: string }>({ type: "get_state" });
      sessionFile = state.sessionFile;
    } catch (error) {
      readError = (error as Error).message;
    }
    // A deliver or stop that happened while we were reading owns the worker now.
    if (shuttingDown || (worker.generation ?? 0) !== generation || worker.process !== process) return;
    worker.sessionFile = sessionFile ?? worker.sessionFile;
    const bounded = text.length > MAX_RESULT ? `${text.slice(0, MAX_RESULT)}\n\n[truncated; full transcript in the session file]` : text;
    worker.tool = undefined;
    if (readError || worker.error) {
      worker.state = "failed";
      worker.error = readError ? `could not read the result: ${readError}` : worker.error;
      worker.awaiting = false;
      save(worker);
      notify(worker, "failed", worker.error!);
      return;
    }
    if (!worker.awaiting) {
      // A settle we did not ask for: the child's own events woke it. Report,
      // but keep the answer to our last message as the result.
      worker.state = aborted ? "stopped" : "completed";
      worker.tool = undefined;
      worker.question = undefined;
      save(worker);
      notify(worker, "update", bounded || "(no text)");
      return;
    }
    worker.result = bounded;
    worker.question = undefined;
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
    await ensureRegistry();
    if (shuttingDown) throw new Error("worker runtime is shutting down");
    const config = loadConfig();
    maxWidgetRows = config.widgetRows;
    const resolved = resolveModel(input.model, config);
    if (resolved.error) throw new Error(resolved.error);
    if (!input.model && ctx.model) {
      resolved.model = `${ctx.model.provider}/${ctx.model.id}`;
      resolved.thinking = ctx.thinkingLevel;
    }
    const cwd = path.resolve(ctx.cwd, input.cwd ?? ".");
    if (!existsSync(cwd)) throw new Error(`cwd does not exist: ${cwd}`);
    const childDepth = depth() + 1;
    if (childDepth > config.maxDepth) throw new Error(`worker nesting limit reached (maxDepth ${config.maxDepth} in workers.json)`);
    const id = `w-${randomBytes(8).toString("hex")}`;
    const sessionDir = path.join(sessionsRoot(), id);
    mkdirSync(sessionDir, { recursive: true });
    const worker: Worker = {
      id, label: label(input.label), task: input.task, cwd, model: resolved.model, thinking: resolved.thinking,
      tools: input.tools, system: input.system?.trim() || undefined, sessionDir,
      state: "running", startedAt: Date.now(), updatedAt: Date.now(), lastEventAt: Date.now(), tokens: 0, calls: 0, awaiting: true, generation: 0,
    };
    const args = ["--session-dir", sessionDir, ...launchArgs(worker, childDepth, config.maxDepth)];
    const child = new PiProcess({ cwd, args, env: { DOTS_WORKER_ID: id, DOTS_WORKER_DEPTH: String(childDepth), DOTS_WORKER_ROOT: rootId() ?? id } });
    attach(worker, child);
    workers.set(id, worker);
    try {
      const disposition = await child.call<{ disposition: string }>({ type: "prompt", message: input.task });
      if (shuttingDown || worker.process !== child) throw new Error("worker startup cancelled by stop or shutdown");
      if (disposition.disposition !== "started" && disposition.disposition !== "queued") throw new Error("the child handled the prompt without starting a run");
    } catch (error) {
      // Never leave a ghost: record the failure and end the process.
      const diagnostic = `${(error as Error).message}${child.stderr.length ? `; stderr: ${child.stderr.join("").trim().slice(-500)}` : ""}`;
      if (!shuttingDown && worker.process === child) {
        worker.process = undefined;
        worker.state = "failed";
        worker.awaiting = false;
        worker.error = diagnostic;
        save(worker);
      }
      await child.close(1000);
      throw new Error(`${worker.id} failed to start: ${diagnostic}`);
    }
    try {
      const state = await child.call<{ sessionFile?: string }>({ type: "get_state" });
      if (!shuttingDown && worker.process === child) worker.sessionFile = state.sessionFile;
    } catch {
      // settle() reads it again
    }
    save(worker);
    return worker;
  };

  /** Deliver text to a worker we own, however it is currently occupied. */
  const deliver = (worker: Worker, text: string, onSending?: () => void): Promise<string> => inOrder(worker, async () => {
    refreshQuestions(worker);
    if (!worker.process) {
      if (!worker.sessionFile) throw new Error(`${worker.id} has no session to continue`);
      const childDepth = depth() + 1;
      const config = loadConfig();
      if (childDepth > config.maxDepth) throw new Error("worker nesting limit reached");
      const child = new PiProcess({
        cwd: worker.cwd,
        args: ["--session", worker.sessionFile, ...launchArgs(worker, childDepth, config.maxDepth)],
        env: { DOTS_WORKER_ID: worker.id, DOTS_WORKER_DEPTH: String(childDepth), DOTS_WORKER_ROOT: rootId() ?? worker.id },
      });
      attach(worker, child);
    }
    const child = worker.process;
    if (!child) throw new Error(`${worker.id} has no active process`);
    onSending?.(); // register this delivery's waiter only after older transitions
    const question = worker.question;
    const wasRunning = worker.state === "running";
    worker.generation = (worker.generation ?? 0) + 1;
    worker.state = "running";
    worker.awaiting = true;
    worker.question = undefined;
    worker.error = undefined;
    worker.lastEventAt = Date.now();
    if (!wasRunning) worker.result = undefined;
    save(worker);
    try {
      if (question) {
        child.respondUi(question.uiId, uiAnswer(question.method, text));
        worker.questions?.shift();
        const nextQuestion = refreshQuestions(worker);
        if (nextQuestion) worker.state = "asking";
        save(worker);
        if (nextQuestion) notify(worker, "asking", `${nextQuestion.text}\n\nAnswer the next pending question with message { to: "${worker.id}", text }.`);
        return "sent an answer to its question";
      }
      // prompt+steer is atomic at the host: it starts an idle child or steers
      // a busy one, including a child that settled just before this request.
      const reply = await child.call<{ disposition: string }>({ type: "prompt", message: text, streamingBehavior: "steer" });
      if (shuttingDown || worker.process !== child) throw new Error(`${worker.id} delivery cancelled by stop or shutdown`);
      if (reply.disposition !== "started" && reply.disposition !== "queued") throw new Error("the child handled the prompt without starting a run");
      return reply.disposition === "queued" ? "steered" : "continued in its own session";
    } catch (error) {
      // Do not roll back a stop, shutdown, or newer owner. Failed/ambiguous
      // delivery retires this process so a retry cannot duplicate queued work.
      if (!shuttingDown && worker.process === child) {
        worker.process = undefined;
        worker.state = "failed";
        worker.awaiting = false;
        worker.error = `delivery failed: ${(error as Error).message}`;
        save(worker);
        wake(worker, "failed", worker.error);
      }
      await child.close(1000);
      throw error;
    }
  });

  /** Block until a worker (or any) settles, asks, or messages; undefined on timeout/abort. */
  const waitFor = (target: Worker | undefined, timeoutMs: number, signal?: AbortSignal) =>
    new Promise<{ worker: Worker; kind: EventKind; body: string } | undefined>((resolve) => {
      if (shuttingDown) { resolve(undefined); return; }
      const finish = (value: { worker: Worker; kind: EventKind; body: string } | undefined) => {
        clearTimeout(timer);
        waiters.delete(waiter);
        cancelWaiters.delete(onAbort);
        signal?.removeEventListener("abort", onAbort);
        resolve(value);
      };
      const timer = setTimeout(() => finish(undefined), timeoutMs);
      const waiter = (worker: Worker, kind: EventKind, body: string): boolean => {
        if (target && worker.id !== target.id) return false;
        finish({ worker, kind, body });
        return true;
      };
      const onAbort = () => finish(undefined);
      waiters.add(waiter);
      cancelWaiters.add(onAbort);
      if (signal?.aborted) {
        finish(undefined);
        return;
      }
      signal?.addEventListener("abort", onAbort);
    });

  /** Stop every live worker this session owns; descendants end through each child's own shutdown. */
  const stopAll = async (): Promise<{ stopped: Worker[]; closed: string[] }> => {
    const live = [...workers.values()].filter((w) => w.state === "running" || w.state === "asking" || (subtree(w)?.live ?? 0) > 0);
    // Idle retained owners may have descendants, or wake again later. Close
    // every owned process, preserving completed/failed results and states.
    const owned = [...workers.values()].filter((w) => w.process || stopping.has(w) || live.includes(w));
    const closed = owned.filter((w) => w.process || stopping.has(w)).map((w) => w.id);
    await Promise.all(owned.map((w) => stop(w, w.state !== "running" && w.state !== "asking")));
    return { stopped: live, closed };
  };

  const stopAllText = ({ stopped, closed }: { stopped: Worker[]; closed: string[] }): string => [
    closed.length ? `Closed ${closed.length} owned process${closed.length === 1 ? "" : "es"}.` : "No owned processes needed closing.",
    stopped.length ? `Stopped active work: ${stopped.map((w) => `${w.label} [${w.id}]`).join(", ")}.` : "No active work remained.",
    "Finished results and saved sessions are retained; message can continue them.",
  ].join(" ");

  const stop = (worker: Worker, preserveState = false): Promise<void> => {
    const existing = stopping.get(worker);
    if (existing) return existing;
    epochs.set(worker, (epochs.get(worker) ?? 0) + 1);
    worker.generation = (worker.generation ?? 0) + 1;
    if (!preserveState) worker.state = "stopped";
    worker.question = undefined;
    worker.questions = [];
    worker.tool = undefined;
    worker.awaiting = false;
    const process = worker.process;
    worker.process = undefined; // no late exit or settle can touch this worker now
    save(worker);
    wake(worker, "stopped", "stopped");
    // Close is the stop boundary, not an acknowledgement from a busy model.
    const closing = process?.close(2000) ?? Promise.resolve();
    stopping.set(worker, closing);
    const clean = () => { if (stopping.get(worker) === closing) stopping.delete(worker); };
    void closing.then(clean, clean);
    return closing;
  };

  const summary = (worker: Worker) => ({
    id: worker.id,
    label: worker.label,
    state: worker.state,
    processState: stopping.has(worker) ? "stopping" : worker.process?.alive ? "alive" : "closed",
    elapsed: elapsed(worker.startedAt),
    tokens: worker.tokens,
    calls: worker.calls,
    tool: worker.tool,
    activity: worker.activity,
    lastFailure: worker.failed,
    quietFor: worker.state === "running" ? elapsed(worker.lastEventAt) : undefined,
    children: subtree(worker),
    recent: withRegistry((r) => r.events(worker.id, 8))?.reverse().map((e) => `${e.kind}: ${e.text}`),
    question: worker.question?.text,
    pendingQuestions: worker.questions?.map((question) => question.text),
    cwd: worker.cwd,
    model: worker.model,
    session: worker.sessionFile,
    result: worker.result,
    error: worker.error,
  });

  pi.registerMessageRenderer<EventDetails>("dots.worker.event", renderEvent);

  pi.on("before_agent_start", (_event, ctx) => track(ctx));
  pi.on("agent_end", (_event, ctx) => track(ctx));

  pi.on("session_start", async (_event, ctx) => {
    track(ctx);
    try { maxWidgetRows = loadConfig().widgetRows; }
    catch (error) { if (ctx.hasUI) ctx.ui.notify((error as Error).message, "warning"); }
    // Children died with the previous process; their sessions persist and can be continued.
    for (const entry of ctx.sessionManager.getBranch()) {
      if (entry.type !== "custom" || entry.customType !== ENTRY) continue;
      const data = entry.data as ReturnType<typeof persisted>;
      const existing = workers.get(data.id);
      if (existing?.process) continue;
      const state: State = data.state === "running" || data.state === "asking" ? "paused" : data.state;
      workers.set(data.id, { ...data, state, question: undefined, tokens: data.tokens ?? 0, calls: data.calls ?? 0, lastEventAt: Date.now() });
    }
    await ensureRegistry();
    if (shuttingDown) return;
    for (const worker of workers.values()) observe(worker);
    widget();
  });

  pi.on("session_shutdown", async () => {
    // Stop UI callbacks before awaiting children: reload invalidates this ctx.
    if (shuttingDown) return;
    shuttingDown = true;
    if (ticker) clearInterval(ticker);
    ticker = undefined;
    requestRender = undefined;
    widgetMounted = false;
    for (const cancel of [...cancelWaiters]) cancel();
    const closing = [...workers.values()].flatMap((worker) => {
      const pending = stopping.get(worker);
      return pending ? [pending] : [];
    });
    for (const worker of workers.values()) {
      const process = worker.process;
      if (!process) continue;
      worker.process = undefined;
      worker.state = worker.state === "running" || worker.state === "asking" ? "paused" : worker.state;
      pi.appendEntry(ENTRY, persisted(worker));
      observe(worker);
      closing.push(process.close(3000));
    }
    context = undefined;
    await Promise.all(closing);
    await registryReady;
    withRegistry((r) => r.close());
    registry = undefined;
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
      "Send text to another session: a worker you spawned (by id or label), a sibling worker (by id; the owner relays it), or \"parent\" when you are a worker. Delivery adapts to the target: it answers a worker that is blocked on a question, steers a running one between tool calls, or continues a finished, stopped, or paused one in its own session. wait: true blocks until an owned worker replies or settles, or the parent answers; sibling relays do not support waiting. Use it for questions that need an answer before you can proceed.",
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
          if (parentQuestionPending) throw new Error("A parent question is already awaiting an answer; combine questions or wait for it first");
          parentQuestionPending = true;
          try {
            const answer = await ctx.ui.input(params.text, undefined, { signal, timeout: params.timeoutMs ?? 600_000 });
            return { content: [{ type: "text", text: answer === undefined ? "The parent did not answer (cancelled or timed out). No decision was approved; report the blocker or ask again." : answer }], details: { answered: answer !== undefined } };
          } finally { parentQuestionPending = false; }
        }
        ctx.ui.notify(`${MARK}${JSON.stringify({ to: "parent", text: params.text })}`, "info");
        return { content: [{ type: "text", text: "sent to parent" }], details: { to: "parent" } };
      }
      let worker: Worker | undefined;
      try {
        worker = find(params.to);
      } catch (error) {
        if (!me) throw error;
        // The relay has no correlated reply channel; do not pretend to wait.
        if (params.wait) throw new Error("wait: true is not supported for sibling relays; ask the parent to coordinate the reply");
        // Not ours: a sibling. The owner routes it through its registry.
        ctx.ui.notify(`${MARK}${JSON.stringify({ to: params.to, text: params.text })}`, "info");
        return { content: [{ type: "text", text: `sent to ${params.to} via the parent` }], details: { to: params.to, relayed: true } };
      }
      const cancellation = new AbortController();
      const waitSignal = signal ? AbortSignal.any([signal, cancellation.signal]) : cancellation.signal;
      const waiting: { pending?: ReturnType<typeof waitFor> } = {};
      let disposition: string;
      try {
        disposition = await deliver(worker, params.text, () => {
          if (params.wait) waiting.pending = waitFor(worker, params.timeoutMs ?? 600_000, waitSignal);
        });
      } catch (error) {
        cancellation.abort();
        throw error;
      }
      if (!waiting.pending) {
        return { content: [{ type: "text", text: `${worker.id}: ${disposition}` }], details: summary(worker) };
      }
      const outcome = await waiting.pending;
      if (!outcome) return { content: [{ type: "text", text: `${worker.id}: ${disposition}; no reply within the wait window (still ${worker.state})` }], details: summary(worker) };
      return { content: [{ type: "text", text: `${worker.id} ${outcome.kind}\n\n${outcome.body}` }], details: summary(worker) };
    },
  });

  pi.registerTool({
    name: "worker_wait",
    label: "Wait for worker",
    description: "Block until a worker settles, asks a question, or sends a message. id: that worker; no id: the first of this session's workers to do so (call again for the next); all: true: until every live worker has settled, returning each result. Use when you have nothing else to do; an interactive parent does not need this because events start its next turn.",
    parameters: Type.Object({
      id: Type.Optional(Type.String()),
      all: Type.Optional(Type.Boolean({ description: "Wait for every live worker of this session; a question from any of them returns early so it can be answered" })),
      timeoutMs: Type.Optional(Type.Number({ description: "Give up after this long (default 600000); the worker keeps running" })),
    }),
    async execute(_id, params, signal, _update, ctx) {
      track(ctx);
      const live = (w: Worker) => w.state === "running";
      if (params.all) {
        const asking = [...workers.values()].find((w) => w.state === "asking");
        if (asking) return { content: [{ type: "text", text: `${asking.id} asking (answer it, then wait again)\n\n${asking.question?.text ?? ""}` }], details: summary(asking) };
        const deadline = Date.now() + (params.timeoutMs ?? 600_000);
        const seen: Worker[] = [];
        while ([...workers.values()].some(live)) {
          const outcome = await waitFor(undefined, Math.max(1, deadline - Date.now()), signal);
          if (!outcome) break;
          if (outcome.kind === "asking" || outcome.kind === "message") {
            return { content: [{ type: "text", text: `${outcome.worker.id} ${outcome.kind} (answer it, then wait again)\n\n${outcome.body}` }], details: summary(outcome.worker) };
          }
          if (!seen.includes(outcome.worker)) seen.push(outcome.worker);
        }
        const now = Date.now();
        const still = [...workers.values()].filter(live);
        const text = [
          ...seen.map((w) => `${w.id} ${w.label} ${w.state}\n\n${w.result ?? w.error ?? ""}`.trimEnd()),
          still.length ? `Still running at the wait window:\n${still.map((w) => describe(w, now, false, 0)).join("\n")}` : "",
        ].filter(Boolean).join("\n\n---\n\n");
        return { content: [{ type: "text", text: text || "No live workers." }], details: [...seen, ...still].map(summary) };
      }
      const target = params.id ? find(params.id) : undefined;
      const candidates = target ? [target] : [...workers.values()];
      if (candidates.length === 0) return { content: [{ type: "text", text: "No workers in this session." }], details: [] };
      const already = candidates.find((w) => !live(w));
      if (already && (target || candidates.every((w) => !live(w)))) {
        return { content: [{ type: "text", text: `${already.id} is ${already.state}${already.result ? `\n\n${already.result}` : ""}${already.question ? `\n\nasking: ${already.question.text}` : ""}` }], details: summary(already) };
      }
      const outcome = await waitFor(target, params.timeoutMs ?? 600_000, signal);
      if (!outcome) {
        const now = Date.now();
        const rows = candidates.filter(live).map((w) => describe(w, now, false, 0));
        return { content: [{ type: "text", text: `No change within the wait window; still running:\n${rows.join("\n")}` }], details: candidates.map(summary) };
      }
      return { content: [{ type: "text", text: `${outcome.worker.id} ${outcome.kind}\n\n${outcome.body}` }], details: summary(outcome.worker) };
    },
  });

  /**
   * The model-facing line for a worker: what it is, how long, and what it is
   * doing right now (or why it is not), with descendants from the registry.
   */
  const describeLine = (w: Worker, now: number): string => {
    const since = now - w.lastEventAt;
    const parts = [`${w.state} · age ${duration(now - w.startedAt)}`, `${compact(w.tokens)} tok`, `${w.calls} call${w.calls === 1 ? "" : "s"}`];
    if (w.state === "running") {
      parts.push(w.tool ? `now: ${label(w.activity ?? w.tool, 80)} (${duration(since)} in)` : w.activity ? `last: ${label(w.activity, 80)} (${duration(since)} ago)` : `no events yet (${duration(since)})`);
      if (since >= STALL_MS) parts.push(`QUIET ${duration(since)}`);
      if (w.failed) parts.push(`last call failed: ${w.failed}`);
    }
    if (w.state === "asking") parts.push(`asking (${duration(since)} ago): ${label(w.question?.text ?? "", 200)}`);
    if (w.state !== "running" && w.state !== "asking") parts.push(stopping.has(w) ? "process stopping" : w.process?.alive ? "process idle" : "process closed");
    return `${w.id}  ${w.label} \u00b7 ${parts.join(" \u00b7 ")}`;
  };
  const describeDescendants = (parent: string, depth: number, now: number, seen = new Set<string>()): string[] => {
    if (seen.has(parent)) return [];
    seen.add(parent);
    return (withRegistry((r) => r.children(parent)) ?? []).flatMap((r) => {
      if (seen.has(r.id)) return [];
      const since = now - r.last_event_at;
      const doing = r.state === "running" ? (r.activity ? ` \u00b7 ${label(r.activity, 70)} (${duration(since)} ago)` : "") : r.state === "asking" ? ` \u00b7 asking: ${label(r.question ?? "", 120)}` : "";
      const line = `${"  ".repeat(depth)}\u2937 ${r.id}  ${r.label} \u00b7 ${r.state} · age ${duration(now - r.started_at)} \u00b7 ${compact(r.tokens)} tok${doing}`;
      return [line, ...describeDescendants(r.id, depth + 1, now, seen)];
    });
  };
  const describe = (w: Worker, now: number, detail: boolean, tailLines: number): string => {
    let out = describeLine(w, now);
    const kids = describeDescendants(w.id, 1, now);
    if (kids.length) out += `\n${kids.join("\n")}`;
    if (!detail && w.result) out += `\n  result: ${label(w.result.split("\n").find((line) => line.trim()) ?? "", 120)}`;
    if (w.error) out += `\n  error: ${w.error}`;
    if (w.sessionFile) out += `\n  session: ${w.sessionFile}`;
    if (detail) {
      const trail = withRegistry((r) => r.events(w.id, 30))?.reverse() ?? [];
      if (trail.length) out += `\n\nactivity (oldest first):\n${trail.map((e) => `  ${new Date(e.at).toISOString().slice(11, 19)} ${e.kind.padEnd(6)} ${label(e.text, 120)}`).join("\n")}`;
      const tail = transcriptTail(w.sessionFile, tailLines);
      if (tail.length) out += `\n\ntranscript tail:\n${tail.join("\n")}`;
      if (w.result) out += `\n\nresult:\n${w.result}`;
    }
    return out;
  };
  /** A descendant we do not own: read-only view from the registry. */
  const describeForeign = (id: string, now: number, tailLines: number): string | undefined => {
    const r = withRegistry((registry) => { registry.sweep(); return registry.get(id); });
    if (!r) return undefined;
    const seen = new Set([id]);
    let parent = r.parent;
    while (parent && !workers.has(parent) && !seen.has(parent)) {
      seen.add(parent);
      parent = withRegistry((registry) => registry.get(parent!))?.parent ?? null;
    }
    if (!parent || !workers.has(parent)) return undefined;
    const tail = transcriptTail(r.session_file ?? undefined, tailLines);
    const trail = withRegistry((registry) => registry.events(r.id, 30))?.reverse() ?? [];
    return [
      `${r.id}  ${r.label} \u00b7 ${r.state} · age ${duration(now - r.started_at)} \u00b7 ${compact(r.tokens)} tok \u00b7 ${r.calls} calls${r.activity ? ` \u00b7 last: ${label(r.activity, 80)} (${duration(now - r.last_event_at)} ago)` : ""}`,
      `  owned by ${r.parent ?? "another session"}; to steer or stop it, message its owner.`,
      r.session_file ? `  session: ${r.session_file}` : "",
      r.question ? `  asking: ${r.question}` : "",
      r.failed ? `  last call failed: ${r.failed}` : "",
      tail.length ? `\ntranscript tail:\n${tail.join("\n")}` : "",
      "  final result is owned by its parent; inspect the session transcript for complete history.",
      trail.length ? `\nactivity (oldest first):\n${trail.map((e) => `  ${new Date(e.at).toISOString().slice(11, 19)} ${e.kind.padEnd(6)} ${label(e.text, 120)}`).join("\n")}` : "",
    ].filter(Boolean).join("\n");
  };

  pi.registerTool({
    name: "worker_status",
    label: "Worker status",
    description: "List this session's workers and their live descendants (what each is doing right now, how long since its last event, QUIET when stalled, last failed call, pending question), or inspect one by id: its activity trail, transcript tail, and full result. A descendant's id shows read-only registry activity and transcript tail; its final result belongs to its owner. Message its owner to steer it. Completions arrive as events; use this to look inside, not to wait.",
    parameters: Type.Object({
      id: Type.Optional(Type.String()),
      tail: Type.Optional(Type.Number({ description: "Recent transcript lines to include for one worker (default 20)" })),
    }),
    async execute(_id, params, _signal, _update, ctx) {
      track(ctx);
      await ensureRegistry();
      if (shuttingDown) throw new Error("worker runtime is shutting down");
      const now = Date.now();
      if (params.id) {
        let own: Worker | undefined;
        try { own = find(params.id); } catch (error) {
          const foreign = describeForeign(params.id.trim(), now, params.tail ?? 20);
          if (foreign) return { content: [{ type: "text", text: foreign }], details: withRegistry((r) => r.get(params.id!.trim())) };
          throw error;
        }
        return { content: [{ type: "text", text: describe(own, now, true, params.tail ?? 20) }], details: summary(own) };
      }
      const rows = [...workers.values()];
      const text = rows.length === 0 ? "No workers in this session." : rows.map((w) => describe(w, now, false, 0)).join("\n");
      return { content: [{ type: "text", text }], details: rows.map(summary) };
    },
  });

  pi.registerTool({
    name: "worker_stop",
    label: "Stop worker",
    description: "Abort a worker and end its process (and, through its own shutdown, its descendants). Its session file stays; message can continue it later. all: true closes every owned process, including idle owners and their descendants, while preserving finished results.",
    parameters: Type.Object({ id: Type.Optional(Type.String()), all: Type.Optional(Type.Boolean()) }),
    async execute(_id, params, _signal, _update, ctx) {
      track(ctx);
      if (params.all) {
        const outcome = await stopAll();
        return { content: [{ type: "text", text: stopAllText(outcome) }], details: { stopped: outcome.stopped.map(summary), closed: outcome.closed } };
      }
      if (!params.id) throw new Error("worker_stop needs id or all: true");
      const worker = find(params.id);
      await stop(worker);
      return { content: [{ type: "text", text: `${worker.id} stopped. ${worker.sessionFile ? `Saved session retained at ${worker.sessionFile}; message can continue it.` : "No saved session is available to resume."}` }], details: summary(worker) };
    },
  });

  pi.registerCommand("workers", {
    description: "List this session's workers; `/workers <id>` shows what one has been doing; `/workers stop` stops them all",
    handler: async (args, ctx) => {
      if (!ctx.hasUI) return;
      const wanted = args.trim();
      if (wanted === "stop" || wanted === "stop-all") {
        const outcome = await stopAll();
        if (!shuttingDown) ctx.ui.notify(stopAllText(outcome), "info");
        return;
      }
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
