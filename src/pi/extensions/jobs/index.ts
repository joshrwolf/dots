import { join, resolve } from "node:path";
import { Type } from "typebox";
import { getAgentDir, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import { ToolCard, OutputView } from "../../src/index.ts";
import { CompletionDelivery, registerDeliveryHooks } from "./delivery.ts";
import { JobsRuntime } from "./runtime.ts";
import { JobsPresentation } from "./ui.ts";
import { JobSchema, JobsSchema, jobOutput } from "./results.ts";
import { ENTRY_TYPE, MAX_RETAINED, MAX_TIMEOUT_MS, RECORD_TYPE, restore, summary, type Job } from "./protocol.ts";

export default function jobs(pi: ExtensionAPI): void {
  let runtime: JobsRuntime | undefined;
  let context: ExtensionContext | undefined;
  let delivery: CompletionDelivery | undefined;
  let presentation: JobsPresentation | undefined;
  const humanStops = new Set<string>();
  const current = (ctx: ExtensionContext): JobsRuntime => {
    if (ctx.mode !== "tui" && ctx.mode !== "rpc") throw new Error("Jobs require a kept-alive TUI or RPC session; print/JSON one-shot modes are unsupported");
    if (!runtime) throw new Error("Jobs session is not active");
    return runtime;
  };
  const initialize = (ctx: ExtensionContext): void => {
    presentation?.dispose();
    humanStops.clear();
    context = ctx;
    delivery = new CompletionDelivery(pi, () => context);
    const restored = new Map<string, Job>();
    for (const entry of ctx.sessionManager.getBranch()) {
      if (entry.type !== "custom" || entry.customType !== ENTRY_TYPE) continue;
      const job = restore(entry.data);
      if (job) {
        restored.set(job.id, job);
        if (restored.size > MAX_RETAINED) {
          const oldest = restored.keys().next().value;
          if (oldest) restored.delete(oldest);
        }
      }
    }
    if (runtime) runtime.replaceHistory([...restored.values()]);
    else runtime = new JobsRuntime({
      root: join(getAgentDir(), "dots", "jobs"),
      persist: (job) => pi.appendEntry(ENTRY_TYPE, job),
      terminal: (job) => {
        // A human stop records its eventual outcome without buying a model turn.
        if (humanStops.delete(job.id)) pi.sendMessage({ customType: RECORD_TYPE, content: summary(job), display: true, details: { id: job.id, state: job.state } }, { triggerTurn: false });
        else delivery?.record(job);
        presentation?.refresh();
      },
      error: (message) => { if (context?.hasUI) context.ui.notify(message, "error"); },
    }, [...restored.values()]);
    const owned = runtime;
    presentation = new JobsPresentation(ctx, {
      list: () => owned.list(), get: (id) => owned.get(id), hasLive: () => owned.hasLive(),
      stop: (id) => {
        humanStops.add(id);
        try { return owned.stop(id); }
        catch (error) { humanStops.delete(id); throw error; }
      },
    });
    presentation.refresh();
  };
  pi.on("session_start", (_event, ctx) => initialize(ctx));
  pi.on("session_shutdown", async () => {
    presentation?.dispose(false);
    presentation = undefined;
    humanStops.clear();
    delivery?.close();
    await runtime?.shutdown();
    runtime = undefined;
    context = undefined;
  });
  pi.on("session_before_tree", (_event, ctx) => {
    if (!runtime?.hasLive()) return undefined;
    if (ctx.hasUI) ctx.ui.notify("Stop live jobs before navigating the session tree.", "warning");
    return { cancel: true };
  });
  pi.on("session_tree", (_event, ctx) => { delivery?.close(); initialize(ctx); });
  registerDeliveryHooks(pi, () => delivery);

  pi.registerTool({
    name: "job_start", label: "Job start",
    description: "Start a finite session-owned shell job on macOS without waiting. Four active slots; timeoutMs defaults to 30 minutes, maximum 24 hours. Caller abort after admission does not stop it. Output prefix/tail paths are returned; use read for output, job_status for facts, job_stop to stop. Exiting/reloading the session stops jobs. No PTY, stdin, service readiness, or universal process containment.",
    parameters: Type.Object({ command: Type.String({ minLength: 1, maxLength: 8192 }), cwd: Type.Optional(Type.String()), timeoutMs: Type.Optional(Type.Integer({ minimum: 1, maximum: MAX_TIMEOUT_MS })) }),
    outputSchema: JobSchema,
    async execute(_id, args, signal, _update, ctx) {
      const job = current(ctx).start(args.command, resolve(ctx.cwd, args.cwd ?? "."), args.timeoutMs, signal);
      presentation?.refresh();
      const data = jobOutput(job);
      return { content: [{ type: "text", text: summary(job) }], details: data, structuredContent: data };
    },
  });
  pi.registerTool({
    name: "job_status", label: "Job status",
    description: "Inspect a session-owned job, or list retained jobs. Root outcome, stop intent, cleanup evidence and capture completeness are separate. Unknown historical jobs are never resumed. Use read on the generated output paths for deeper output.",
    parameters: Type.Object({ id: Type.Optional(Type.String()) }),
    outputSchema: JobsSchema,
    async execute(_id, args, _signal, _update, ctx) {
      const r = current(ctx);
      const jobs = args.id ? [r.get(args.id)] : r.list();
      const data = { jobs: jobs.map(jobOutput) };
      return { content: [{ type: "text", text: jobs.map(summary).join("\n\n") || "No retained jobs." }], details: data, structuredContent: data };
    },
  });
  pi.registerTool({
    name: "job_stop", label: "Job stop",
    description: "Request idempotent TERM then fixed 3-second group KILL for an owned job. Returns stopping until exit is observed; cannot control historical jobs. Does not signal persisted numeric PIDs.",
    parameters: Type.Object({ id: Type.String() }),
    outputSchema: JobSchema,
    async execute(_id, args, _signal, _update, ctx) {
      const job = current(ctx).stop(args.id);
      presentation?.refresh();
      const data = jobOutput(job);
      return { content: [{ type: "text", text: summary(job) }], details: data, structuredContent: data };
    },
  });
  pi.registerCommand("jobs", {
    description: "Inspect or stop jobs in the TUI: /jobs [id]; text inspection in RPC/headless modes",
    handler: async (args, ctx) => {
      if (!runtime) throw new Error("Jobs session is not active");
      if (ctx.mode === "tui" && ctx.hasUI) { await presentation?.open(args.trim() || undefined); return; }
      const jobs = args.trim() ? [runtime.get(args.trim())] : runtime.list();
      // Headless/RPC inspection remains a bounded, non-waking transcript record.
      pi.sendMessage({ customType: "dots-jobs-inspection", content: jobs.map(summary).join("\n\n") || "No retained jobs.", display: true }, { triggerTurn: false });
    },
  });
  for (const type of [RECORD_TYPE, "dots-jobs-inspection"]) pi.registerMessageRenderer(type, (message, options, theme) => {
    const text = typeof message.content === "string" ? message.content : message.content.filter((b) => b.type === "text").map((b) => b.text).join("\n");
    const card = new ToolCard();
    card.update({
      heading: [{ text: "jobs", tone: "toolTitle", bold: true, priority: 100, shrink: "clip" }],
      status: [], notices: [], body: new OutputView(text), caption: "",
    }, theme);
    return { render: (width) => card.render(Math.max(0, width - options.outputPad * 2)).map((line) => " ".repeat(options.outputPad) + line), invalidate: () => card.invalidate() };
  });
}
