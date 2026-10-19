import path from "node:path";
import { Type } from "typebox";
import type { ExtensionAPI, ExtensionContext, Theme } from "@earendil-works/pi-coding-agent";
import { labelText, sourceText } from "../../src/index.ts";
import { GoalWidget, GoalInspector, inspection, orientation, type Focus, type GoalView } from "./present.ts";
import { dotsSource, type GoalSource } from "./source.ts";

export const FOCUS_ENTRY = "dots.goal.focus";
const WIDGET = "dots.goal";
const HELP = "/goal [new IDEA | show | history | focus [ID] | list | refresh | unfocus]. New starts a planning conversation; focus/inspection never starts work.";

export function restoreFocus(entries: readonly { type: string; customType?: string; data?: unknown }[]): Focus | undefined {
  for (const entry of [...entries].reverse()) {
    if (entry.type !== "custom" || entry.customType !== FOCUS_ENTRY) continue;
    const value = entry.data as { version?: unknown; focus?: unknown } | undefined;
    if (value?.version !== 1) throw new Error("Unsupported saved goal focus; use /goal unfocus or focus ID.");
    if (value.focus === null) return undefined;
    const candidate = value.focus as Partial<Focus> | undefined;
    if (!candidate || typeof candidate.id !== "string" || typeof candidate.project !== "string"
      || !/^t-[a-f0-9]+$/.test(candidate.id) || !candidate.project) throw new Error("Invalid saved goal focus; use /goal unfocus or focus ID.");
    return { id: candidate.id, project: candidate.project };
  }
  return undefined;
}

export default function goalExtension(pi: ExtensionAPI, source: GoalSource = dotsSource): void {
  let focus: Focus | undefined;
  let model: GoalView | undefined;
  let generation = 0;
  let active = false;
  let request: { controller: AbortController; pending: boolean; result: Promise<GoalView | undefined> } | undefined;
  let dialog: AbortController | undefined;
  let closeInspector: (() => void) | undefined;
  let mounted = false;
  let widget: { view: GoalWidget; theme: Theme; redraw: () => void } | undefined;

  const notify = (ctx: ExtensionContext, message: string, type: "info" | "error" = "info") => {
    if (ctx.hasUI) ctx.ui.notify(sourceText(message).slice(0, 2000) + (message.length > 2000 ? "\n[Display limited; use dots task goal list --json for complete records.]" : ""), type);
    else pi.sendMessage({ customType: "dots.goal.notice", content: message, display: true }, { triggerTurn: false });
  };
  const draw = (ctx: ExtensionContext) => {
    if (!active || ctx.mode !== "tui" || !ctx.hasUI) return;
    if (!model) {
      if (mounted) ctx.ui.setWidget(WIDGET, undefined);
      widget = undefined; mounted = false;
      return;
    }
    if (!mounted) {
      mounted = true;
      const epoch = generation;
      ctx.ui.setWidget(WIDGET, (tui, theme) => {
        const owned = { view: new GoalWidget(), theme, redraw: () => tui.requestRender() };
        widget = owned;
        if (model) owned.view.update(model, theme);
        return {
          render: (width) => active && epoch === generation ? owned.view.render(width) : [],
          invalidate: () => owned.view.invalidate(),
          dispose: () => { if (widget === owned) { widget = undefined; mounted = false; } },
        };
      });
    }
    if (widget && model) { widget.view.update(model, widget.theme); widget.redraw(); }
  };
  const invalidate = (ctx?: ExtensionContext) => {
    generation++;
    request?.controller.abort(); request = undefined;
    dialog?.abort(); dialog = undefined;
    closeInspector?.(); closeInspector = undefined;
    if (ctx && mounted && ctx.mode === "tui" && ctx.hasUI) ctx.ui.setWidget(WIDGET, undefined);
    mounted = false; widget = undefined;
  };

  // Ordinary observers share an in-flight read, including an explicit focus
  // change. Only explicit selection may supersede it. Observers follow that
  // replacement and consume its result, never the previous global model.
  const refresh = async (ctx: ExtensionContext, selected?: string, signal?: AbortSignal): Promise<GoalView | undefined> => {
    const epoch = generation;
    if (!active || signal?.aborted) return undefined;
    if (selected || !request?.pending) {
      const previous = focus;
      const id = selected ?? previous?.id;
      if (!id) return undefined;
      request?.controller.abort();
      const read = { controller: new AbortController(), pending: true, result: Promise.resolve<GoalView | undefined>(undefined) };
      request = read;
      const abort = () => read.controller.abort();
      signal?.addEventListener("abort", abort, { once: true });
      const current = () => active && generation === epoch && request === read && !read.controller.signal.aborted;
      read.result = (async () => {
        try {
          const snapshot = await source.show(ctx.cwd, id, read.controller.signal);
          if (!current()) return undefined;
          if (snapshot.overview.goal.root.id !== id) throw new Error("dots returned a different goal");
          if (!selected && snapshot.project !== previous?.project) throw new Error("Saved goal belongs to a different project; explicitly focus a goal in this project.");
          const brief = await source.brief(snapshot.overview.goal.brief);
          if (!current()) return undefined;
          const next = { id, project: snapshot.project };
          if (selected) pi.appendEntry(FOCUS_ENTRY, { version: 1, focus: next });
          focus = next;
          model = { focus, snapshot, brief };
          draw(ctx);
          return model;
        } catch (error) {
          if (!current()) return undefined;
          const message = String(error);
          if (selected) notify(ctx, message, "error");
          // A failed selection keeps the previous focus, but not its stale
          // observation. Concurrent pre-run readers receive the failure too.
          if (focus) { model = { focus, error: message }; draw(ctx); return model; }
          return undefined;
        } finally { read.pending = false; signal?.removeEventListener("abort", abort); }
      })();
    }
    let observed = request;
    while (observed) {
      const result = await observed.result;
      if (!active || generation !== epoch) return undefined;
      if (request === observed) return result;
      if (selected) return undefined;
      observed = request;
    }
    return undefined;
  };

  const restore = async (ctx: ExtensionContext) => {
    invalidate(ctx); active = true; focus = undefined; model = undefined;
    try { focus = restoreFocus(ctx.sessionManager.getBranch()); }
    catch (error) { notify(ctx, String(error), "error"); }
    if (focus) { model = { focus }; draw(ctx); await refresh(ctx); }
  };
  pi.on("session_start", (_event, ctx) => restore(ctx));
  pi.on("session_tree", (_event, ctx) => restore(ctx));
  pi.on("session_shutdown", () => { active = false; invalidate(); focus = undefined; model = undefined; });
  pi.on("before_agent_start", async (event, ctx) => {
    if (!focus && !request?.pending) return;
    const epoch = generation;
    const observed = await refresh(ctx);
    if (!active || generation !== epoch || !observed) return;
    // A synthetic context-file identity, not a dependency on memory's backing layout.
    const key = path.join(ctx.cwd, ".pi", "goal-context");
    const files = event.systemPromptOptions.contextFiles;
    const index = files.findIndex((file) => file.path === key);
    const block = { path: key, content: orientation(observed) };
    if (index < 0) files.push(block); else files[index] = block;
  });
  pi.on("agent_end", async (_event, ctx) => { await refresh(ctx); });

  const show = async (ctx: ExtensionContext, history = false) => {
    // Reserve the interaction before reading: overlapping commands must not
    // replace a custom screen whose completion promise is still outstanding.
    const controller = new AbortController(); dialog = controller;
    const epoch = generation;
    const current = () => active && generation === epoch && !controller.signal.aborted;
    try {
      let observed = await refresh(ctx);
      if (!current()) return;
      if (!observed) { notify(ctx, "No focused goal. Use /goal focus ID or /goal list."); return; }
      if (history && !observed.error) {
        try {
          const snapshot = await source.show(ctx.cwd, observed.focus.id, controller.signal, true);
          if (!current()) return;
          if (snapshot.project !== observed.focus.project || snapshot.overview.goal.root.id !== observed.focus.id) throw new Error("Goal history identity changed");
          const brief = await source.brief(snapshot.overview.goal.brief);
          if (!current()) return;
          observed = { ...observed, snapshot, brief, history: true };
        } catch (error) {
          if (current()) notify(ctx, String(error), "error");
          return;
        }
      }
      const document = observed;
      if (ctx.mode !== "tui" || !ctx.hasUI) {
        pi.sendMessage({ customType: "dots.goal.inspection", content: inspection(document), display: true }, { triggerTurn: false });
        return;
      }
      await ctx.ui.custom<void>((tui, theme, bindings, done) => {
        if (!current()) { done(); return { render: () => [], invalidate() {} }; }
        const close = () => done();
        closeInspector = close;
        const view = new GoalInspector(bindings, close, () => tui.requestRender(), () => Math.max(1, Math.min(24, tui.terminal.rows - 10)));
        view.setGoal(document, theme, () => tui.requestRender());
        return Object.assign(view, { dispose: () => { if (closeInspector === close) closeInspector = undefined; } });
      });
    } finally { if (dialog === controller) dialog = undefined; }
  };

  pi.registerTool({
    name: "goal_focus",
    label: "Focus goal",
    description: "Focus an existing goal in this Pi session and return its current brief and slice overview. Use after creating/adopting a goal at the user's request. Writes only session focus; does not claim tasks, execute slices, or authorize work. Create goals with dots task goal create TITLE --brief PATH --json; shared tasks remain CLI-owned.",
    parameters: Type.Object({ id: Type.String({ pattern: "^t-[a-f0-9]+$", maxLength: 64 }) }),
    annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    async execute(_callId, { id }, signal, _update, ctx) {
      if (signal?.aborted) throw new Error("Goal focus cancelled");
      if (dialog || closeInspector) throw new Error("A goal dialog is already open");
      const controller = new AbortController(); dialog = controller;
      try {
        const observed = await refresh(ctx, id, signal);
        if (!observed || observed.error || observed.focus.id !== id) throw new Error(observed?.error ?? "Goal focus was interrupted");
        return { content: [{ type: "text", text: inspection(observed) }], details: { id: observed.focus.id, project: observed.focus.project } };
      } finally { if (dialog === controller) dialog = undefined; }
    },
  });

  pi.registerCommand("goal", {
    description: "Plan a new goal, or focus/inspect one: [new IDEA|show|history|focus [ID]|list|refresh|unfocus]",
    handler: async (args, ctx) => {
      const words = args.trim().split(/\s+/).filter(Boolean);
      const action = words[0] ?? (focus ? "show" : "focus");
      if (action === "new") {
        const idea = args.trim().slice(3).trim();
        pi.sendUserMessage([
          "Help me shape a goal" + (idea ? `: ${idea}` : ". Ask what I want to accomplish."),
          "Discuss only what is unclear. When there is enough direction, capture a concise Markdown brief (including our working agreement) in a suitable document, then use dots task goal create TITLE --brief PATH --json to create its root and association together. If an appropriate root already exists, attach the brief instead. Add useful initial slices under that root and call goal_focus with its id so I can see it here.",
          "This request is to establish shared context and tracking, not to start implementation. Do not invent an execution policy or a fixed pipeline. Use the existing slices workflow; Git/PR state is evidence, task status is coordination.",
        ].join("\n\n"), { deliverAs: "followUp" });
        return;
      }
      if (words.length > (action === "focus" ? 2 : 1)) { notify(ctx, HELP, "error"); return; }
      if (action === "unfocus") {
        invalidate(ctx);
        focus = undefined; model = undefined;
        pi.appendEntry(FOCUS_ENTRY, { version: 1, focus: null });
        notify(ctx, "Goal unfocused; shared tasks and brief are unchanged.");
        return;
      }
      if (dialog || closeInspector) { notify(ctx, "A goal dialog is already open."); return; }
      if (action === "show" || action === "history") { await show(ctx, action === "history"); return; }
      if (action === "refresh") {
        const epoch = generation;
        const observed = await refresh(ctx);
        if (!active || generation !== epoch) return;
        notify(ctx, observed ? observed.error ?? "Goal refreshed (local task records only)." : "No focused goal.", observed?.error ? "error" : "info");
        return;
      }
      if (action !== "focus" && action !== "list") { notify(ctx, HELP, "error"); return; }
      const controller = new AbortController(); dialog = controller;
      const epoch = generation;
      const current = () => active && generation === epoch && !controller.signal.aborted;
      try {
        let id = words[1];
        if (!id) {
          const list = await source.list(ctx.cwd, controller.signal);
          if (!current()) return;
          if (action === "list" || !ctx.hasUI) {
            notify(ctx, list.goals.length ? list.goals.map((g) => `${g.root.id} [${g.root.status}] ${g.root.title}`).join("\n") + "\nUse /goal focus ID." : "No open goals. Attach a brief using dots task goal attach ID --brief PATH.");
            return;
          }
          if (!list.goals.length) { notify(ctx, "No open goals. Attach a brief using dots task goal attach ID --brief PATH."); return; }
          const choices = list.goals.map((g) => `${g.root.id} · ${labelText(g.root.title)}`);
          const selected = await ctx.ui.select("Focus a goal (does not start work)", choices, { signal: controller.signal });
          if (!current() || selected === undefined) return;
          id = list.goals[choices.indexOf(selected)]?.root.id;
        }
        if (!id || !/^t-[a-f0-9]+$/.test(id)) { notify(ctx, "Expected a task root ID, such as t-123abc.", "error"); return; }
        const observed = await refresh(ctx, id);
        if (observed && !observed.error && observed.focus.id === id && current()) notify(ctx, `Focused ${id}. No work started. Use /goal to inspect.`);
      } catch (error) { if (current()) notify(ctx, String(error), "error"); }
      finally { if (dialog === controller) dialog = undefined; }
    },
  });
}
