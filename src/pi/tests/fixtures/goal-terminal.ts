// Opt-in neutral fixture: real goal extension/components, no CLI or model calls.
import { appendFileSync } from "node:fs";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import goal from "../../extensions/goal/index.ts";
import { withHierarchy, type GoalSnapshot } from "../../extensions/goal/source.ts";
import { goalFixtures } from "../../dev/goals.ts";

export default function (pi: ExtensionAPI): void {
  const mark = (text: string) => appendFileSync(process.env.DOTS_GOAL_MARKER!, `${text}\n`);
  const root = { id: "t-123abc", title: "Unified fixture exports", status: "open" as const, parent: null,
    children: 1, owner: null, lease_until: null, pr: null, branch: null, blocked_by: [] };
  const snapshot: GoalSnapshot = { project: "/example/project", now: 100,
    overview: { goal: { root, brief: "/example/brief.md" },
      tasks: [{ ...root, id: "t-456def", title: "Reference fixture slice", parent: root.id, children: 0 }],
      hidden_closed: 0,
      progress: { total: 1, open: 1, held: 0, done: 0, dropped: 0, ready: 1, claimed: 0, with_pr: 0 } },
  };
  const observed: ExtensionAPI = { ...pi, registerCommand(name, definition) {
    pi.registerCommand(name, { ...definition, async handler(args, ctx) {
      const ui: typeof ctx.ui = { ...ctx.ui, async custom(factory, options) {
        try {
          return await ctx.ui.custom(async (...parameters) => {
            const view = await factory(...parameters);
            const render = view.render.bind(view);
            view.render = (width) => {
              const lines = render(width);
              mark(`inspector-render:${width}:${lines.length}`);
              return lines;
            };
            return view;
          }, options);
        } finally { mark(`editor:${ctx.ui.getEditorText()}`); mark("inspector-closed"); }
      } };
      try { await definition.handler(args, { ...ctx, ui }); } finally { mark(`command-end:${args}`); }
    } });
  } };
  goal(observed, {
    async list() { return { project: snapshot.project, goals: [snapshot.overview.goal] }; },
    async show(_cwd, id, _signal, history = false) {
      mark("read");
      if (id === "t-abcdef") {
        const nested = goalFixtures(8)[0]!.snapshot!;
        const allTasks = nested.allTasks!.map((task) => ({ ...task, parent: task.parent === nested.overview.goal.root.id ? id : task.parent }));
        return withHierarchy({ ...nested, project: snapshot.project, allTasks,
          overview: { ...nested.overview, goal: { ...nested.overview.goal, root: { ...nested.overview.goal.root, id, title: "Nested fixture goal" } } },
        }, history);
      }
      const finished = id === "t-fedcba";
      return withHierarchy({ ...snapshot, overview: { ...snapshot.overview,
        goal: { ...snapshot.overview.goal, root: { ...root, id, status: finished ? "done" : "open", title: finished ? "Completed fixture goal" : id === root.id ? root.title : "Second fixture goal" } },
        tasks: snapshot.overview.tasks.map((task) => ({ ...task, parent: id, status: finished ? "done" : "open", title: finished ? "Completed fixture slice" : task.title })),
        progress: finished ? { ...snapshot.overview.progress, done: 1, open: 0, ready: 0 } : snapshot.overview.progress,
      } }, history);
    },
    async brief() { return { text: "# Fixture brief\n" + Array.from({ length: 100 }, (_, i) => `Pattern line ${i}: shared ownership, café 👩‍💻`).join("\n") + "\nEND_OF_GOAL_BRIEF" }; },
  });
  pi.on("session_start", () => mark("start"));
  pi.on("agent_start", () => mark("UNEXPECTED_PROVIDER_TURN"));
  pi.on("input", () => ({ action: "handled" }));
  pi.registerCommand("goal-fixture-exit", { description: "Exit isolated fixture", handler: async (_args, ctx) => ctx.shutdown() });
}
