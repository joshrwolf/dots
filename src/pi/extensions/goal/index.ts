import path from "node:path";
import { Type } from "typebox";
import { MouseRegion } from "@earendil-works/pi-tui";
import type { ExtensionAPI, ExtensionContext, Theme } from "@earendil-works/pi-coding-agent";
import { labelText, sourceText } from "../../src/index.ts";
import { FollowedWidget, GoalInspector, inspection, orientation, compactSummary, type Focus, type GoalView } from "./present.ts";
import { dotsSource, type GoalSource } from "./source.ts";

export const FOCUS_ENTRY = "dots.goal.focus";
const WIDGET = "dots.goal";
const HELP = "/goal [new IDEA | follow [ID] | show [ID] | history [ID] | list | followed | refresh | unfollow ID | clear]. focus aliases follow; unfocus without an ID clears all. No shared task state is changed.";

export function restoreGoals(entries: readonly { type: string; customType?: string; data?: unknown }[]): { focus?: Focus; followed: Focus[] } {
  for (const entry of [...entries].reverse()) {
    if (entry.type !== "custom" || entry.customType !== FOCUS_ENTRY) continue;
    const value = entry.data as { version?: unknown; focus?: unknown; followed?: unknown } | undefined;
    if (value?.version !== 1 && value?.version !== 2) throw new Error("Unsupported saved goal focus; use /goal clear.");
    const decode = (raw: unknown): Focus => {
      const candidate = raw as Partial<Focus> | undefined;
      if (!candidate || typeof candidate.id !== "string" || typeof candidate.project !== "string"
        || !/^t-[a-f0-9]+$/.test(candidate.id) || !candidate.project) throw new Error("Invalid saved goal focus; use /goal clear.");
      return { id: candidate.id, project: candidate.project };
    };
    const focus = value.focus === null ? undefined : decode(value.focus);
    if (value.version === 1) return { ...(focus ? { focus } : {}), followed: focus ? [focus] : [] };
    if (!Array.isArray(value.followed)) throw new Error("Invalid saved followed goals; use /goal clear.");
    const followed = value.followed.map(decode);
    if (new Set(followed.map((goal) => goal.id)).size !== followed.length || (focus ? !followed.some((goal) => goal.id === focus.id && goal.project === focus.project) : followed.length > 0)) throw new Error("Invalid saved goal selection; use /goal clear.");
    return { ...(focus ? { focus } : {}), followed };
  }
  return { followed: [] };
}

export function restoreFocus(entries: Parameters<typeof restoreGoals>[0]): Focus | undefined {
  return restoreGoals(entries).focus;
}

export default function goalExtension(pi: ExtensionAPI, source: GoalSource = dotsSource): void {
  let focus: Focus | undefined;
  let model: GoalView | undefined;
  const followed = new Map<string, GoalView>();
  const persist = () => pi.appendEntry(FOCUS_ENTRY, { version: 2, focus: focus ?? null, followed: [...followed.values()].map((view) => view.focus) });
  let generation = 0;
  let active = false;
  let request: { controller: AbortController; pending: boolean; selected: boolean; result: Promise<GoalView | undefined> } | undefined;
  let dialog: AbortController | undefined;
  let closeInspector: (() => void) | undefined;
  let mounted = false;
  let widget: { view: FollowedWidget; theme: Theme; redraw: () => void } | undefined;

  const notify = (ctx: ExtensionContext, message: string, type: "info" | "error" = "info", recovery = "dots task goal list --json for complete shared records") => {
    if (ctx.hasUI) ctx.ui.notify(sourceText(message).slice(0, 2000) + (message.length > 2000 ? `\n[Display limited; use ${recovery}.]` : ""), type);
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
        const owned = { view: new FollowedWidget(), theme, redraw: () => tui.requestRender() };
        widget = owned;
        owned.view.update([...followed.values()], focus?.id, theme);
        const region = new MouseRegion({
          render: (width) => active && epoch === generation ? owned.view.render(width) : [],
          invalidate: () => owned.view.invalidate(),
        }, (event) => {
          if (!active || epoch !== generation || widget !== owned || dialog || closeInspector
            || event.button !== "left" || event.shift || event.alt || event.ctrl) return undefined;
          // Pi synthesizes a click after an accepted press, only when it was not a drag.
          if (event.type === "press") return { handled: true, render: false };
          if (event.type !== "click") return undefined;
          void show(ctx).catch((error) => {
            if (active && epoch === generation && widget === owned) notify(ctx, String(error), "error");
          });
          return { handled: true, render: false };
        });
        return Object.assign(region, {
          dispose: () => { if (widget === owned) { widget = undefined; mounted = false; } },
        });
      });
    }
    if (widget) { widget.view.update([...followed.values()], focus?.id, widget.theme); widget.redraw(); }
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
      const id = selected ?? focus?.id;
      if (!id) return undefined;
      request?.controller.abort();
      const read = { controller: new AbortController(), pending: true, selected: !!selected, result: Promise.resolve<GoalView | undefined>(undefined) };
      request = read;
      const abort = () => read.controller.abort();
      signal?.addEventListener("abort", abort, { once: true });
      const current = () => active && generation === epoch && request === read && !read.controller.signal.aborted;
      read.result = (async () => {
        try {
          const targets = selected ? [id] : [...followed.keys()];
          const observations = await Promise.all(targets.map(async (target): Promise<GoalView> => {
            const saved = followed.get(target)?.focus;
            try {
              const snapshot = await source.show(ctx.cwd, target, read.controller.signal);
              if (snapshot.overview.goal.root.id !== target) throw new Error("dots returned a different goal");
              if (!selected && snapshot.project !== saved?.project) throw new Error("Saved goal belongs to a different project; explicitly follow a goal in this project.");
              if (!current()) throw new Error("Goal read cancelled");
              const brief = await source.brief(snapshot.overview.goal.brief);
              return { focus: { id: target, project: snapshot.project }, snapshot, brief };
            } catch (error) {
              if (selected) throw error;
              return { focus: saved!, error: String(error) };
            }
          }));
          if (!current()) return undefined;
          for (const observation of observations) followed.set(observation.focus.id, observation);
          if (selected) { focus = observations[0]!.focus; persist(); }
          model = focus ? followed.get(focus.id) : undefined;
          draw(ctx);
          return model;
        } catch (error) {
          if (!current()) return undefined;
          const message = String(error);
          if (selected) notify(ctx, message, "error");
          // A rejected addition must not overwrite another goal's observation.
          // Pre-run observers refresh the followed set after selection settles.
          if (focus) return { focus, error: message };
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
    invalidate(ctx); active = true; focus = undefined; model = undefined; followed.clear();
    try {
      const saved = restoreGoals(ctx.sessionManager.getBranch());
      focus = saved.focus;
      for (const item of saved.followed) followed.set(item.id, { focus: item });
    }
    catch (error) { notify(ctx, String(error), "error"); }
    if (focus) { model = { focus }; draw(ctx); await refresh(ctx); }
  };
  pi.on("session_start", (_event, ctx) => restore(ctx));
  pi.on("session_tree", (_event, ctx) => restore(ctx));
  pi.on("session_shutdown", () => { active = false; invalidate(); focus = undefined; model = undefined; followed.clear(); });
  pi.on("before_agent_start", async (event, ctx) => {
    if (!focus && !request?.pending) return;
    // Membership changes invalidate in-flight observations. Retry the remaining
    // set; an unfollow must neither resurrect a goal nor omit the other goals.
    while (active) {
      const epoch = generation;
      await refresh(ctx);
      if (!active) return;
      if (generation !== epoch) { if (followed.size || request?.pending) continue; return; }
      if (!followed.size) return;
      if (request?.selected) continue; // Explicit follow reads only its target.
      break;
    }
    // A synthetic context-file identity, not a dependency on memory's backing layout.
    const key = path.join(ctx.cwd, ".pi", "goal-context");
    const files = event.systemPromptOptions.contextFiles;
    const index = files.findIndex((file) => file.path === key);
    const block = { path: key, content: [...followed.values()].map(orientation).join("\n\n---\n\n") };
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
    description: "Follow an existing goal in this session, additively, and select its details. Every followed goal gets fresh brief/task context before runs. Other goals remain followed; no tasks, claims, or execution state change. Use goal_unfocus to remove one or clear all. Create shared goals with dots task goal create TITLE --brief PATH --json.",
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

  const unfollow = (ctx: ExtensionContext, id?: string) => {
    invalidate(ctx);
    if (id) followed.delete(id); else followed.clear();
    if (!focus || !followed.has(focus.id)) focus = followed.values().next().value?.focus;
    model = focus ? followed.get(focus.id) : undefined;
    persist(); draw(ctx);
    return id ? `Unfollowed ${id}; shared goals, tasks and claims are unchanged.` : "Goal unfocused; all session follows cleared. Shared goals, tasks and claims are unchanged.";
  };
  pi.registerTool({
    name: "goal_unfocus", label: "Unfollow goal",
    description: "Remove one goal from this session's followed set, or clear all follows on request. Removes its UI and future automatic context, not prior conversation. Does not change shared goals, tasks, claims, workers or other sessions. Specify id or all:true.",
    parameters: Type.Object({ id: Type.Optional(Type.String({ pattern: "^t-[a-f0-9]+$", maxLength: 64 })), all: Type.Optional(Type.Boolean()) }),
    annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    async execute(_callId, { id, all }, signal, _update, ctx) {
      if (signal?.aborted) throw new Error("Goal unfocus cancelled");
      if ((!id && all !== true) || (id && all === true)) throw new Error("Specify id or all:true, not both.");
      return { content: [{ type: "text", text: unfollow(ctx, id) }], details: { followed: [...followed.keys()] } };
    },
  });
  pi.registerTool({
    name: "goal_list", label: "List followed goals",
    description: "List this session's followed goal IDs and the selected details view. Read-only session state; use dots task goal list --json for all shared project goals. Selection is not execution state.",
    parameters: Type.Object({}), annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true, openWorldHint: false },
    async execute() {
      const goals = [...followed.values()].map((view) => ({ ...view.focus, title: view.snapshot?.overview.goal.root.title, selected: view.focus.id === focus?.id }));
      return { content: [{ type: "text", text: JSON.stringify({ followed: goals }) }], details: { followed: goals } };
    },
  });

  pi.registerCommand("goal", {
    description: "Follow multiple goals: [new IDEA|follow [ID]|show [ID]|history [ID]|list|followed|refresh|unfollow ID|clear]",
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
      if (words.length > (["focus", "follow", "show", "history", "unfocus", "unfollow"].includes(action) ? 2 : 1)) { notify(ctx, HELP, "error"); return; }
      if (action === "unfocus" || action === "unfollow" || action === "clear") {
        if (action === "unfollow" && !words[1]) { notify(ctx, "Use /goal unfollow ID or /goal clear.", "error"); return; }
        if (words[1] && !/^t-[a-f0-9]+$/.test(words[1])) { notify(ctx, HELP, "error"); return; }
        notify(ctx, unfollow(ctx, words[1])); return;
      }
      if (action === "followed") {
        notify(ctx, followed.size ? [...followed.values()].map((view) => `${view.focus.id === focus?.id ? "> " : "  "}${view.focus.id} ${view.snapshot?.overview.goal.root.title ?? ""} · ${compactSummary(view)}`).join("\n") : "No followed goals.", "info", "goal_list for complete session follows"); return;
      }
      if (dialog || closeInspector) { notify(ctx, "A goal dialog is already open."); return; }
      if (action === "show" || action === "history") {
        if (words[1]) {
          const chosen = followed.get(words[1]);
          if (!chosen) { notify(ctx, "Goal is not followed. Use /goal follow ID first.", "error"); return; }
          focus = chosen.focus; model = chosen; persist(); draw(ctx);
        } else if (followed.size > 1 && ctx.hasUI && ctx.mode === "tui") {
          const controller = new AbortController(); dialog = controller;
          const choices = [...followed.values()].map((view) => `${view.focus.id} · ${labelText(view.snapshot?.overview.goal.root.title ?? view.focus.id)}`);
          try {
            const choice = await ctx.ui.select("Inspect a followed goal", choices, { signal: controller.signal });
            if (controller.signal.aborted || !active || choice === undefined) return;
            const chosen = [...followed.values()][choices.indexOf(choice)];
            if (!chosen) return;
            focus = chosen.focus; model = chosen; persist(); draw(ctx);
          } finally { if (dialog === controller) dialog = undefined; }
        }
        await show(ctx, action === "history"); return;
      }
      if (action === "refresh") {
        const epoch = generation;
        await refresh(ctx);
        if (!active || generation !== epoch) return;
        while (request?.selected && followed.size) {
          await refresh(ctx);
          if (!active || generation !== epoch) return;
        }
        const attention = [...followed.values()].filter((view) => view.error || view.brief?.warning);
        const status = followed.size ? `${followed.size} followed goal(s) refreshed (local records only).` : "No followed goals.";
        notify(ctx, attention.length ? `${status} ${attention.length} need attention.\n${attention.map((view) => `${view.focus.id}: ${view.error ?? view.brief?.warning}`).join("\n")}` : status, attention.some((view) => view.error) ? "error" : "info");
        return;
      }
      if (action !== "focus" && action !== "follow" && action !== "list") { notify(ctx, HELP, "error"); return; }
      const controller = new AbortController(); dialog = controller;
      const epoch = generation;
      const current = () => active && generation === epoch && !controller.signal.aborted;
      try {
        let id = words[1];
        if (!id) {
          const list = await source.list(ctx.cwd, controller.signal);
          if (!current()) return;
          if (action === "list" || !ctx.hasUI) {
            notify(ctx, list.goals.length ? list.goals.map((g) => `${g.root.id} [${g.root.status}]${followed.has(g.root.id) ? " [followed]" : ""} ${g.root.title}`).join("\n") + "\nUse /goal focus ID." : "No open goals. Attach a brief using dots task goal attach ID --brief PATH.");
            return;
          }
          if (!list.goals.length) { notify(ctx, "No open goals. Attach a brief using dots task goal attach ID --brief PATH."); return; }
          const choices = list.goals.map((g) => `${g.root.id} · ${labelText(g.root.title)}`);
          const selected = await ctx.ui.select("Follow a goal (adds context; does not start work)", choices, { signal: controller.signal });
          if (!current() || selected === undefined) return;
          id = list.goals[choices.indexOf(selected)]?.root.id;
        }
        if (!id || !/^t-[a-f0-9]+$/.test(id)) { notify(ctx, "Expected a task root ID, such as t-123abc.", "error"); return; }
        const observed = await refresh(ctx, id);
        if (observed && !observed.error && observed.focus.id === id && current()) notify(ctx, `Focused ${id}; following ${followed.size} goal(s). Other goals are unchanged. No work started. Use /goal to inspect.`);
      } catch (error) { if (current()) notify(ctx, String(error), "error"); }
      finally { if (dialog === controller) dialog = undefined; }
    },
  });
}
