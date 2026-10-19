// Opt-in neutral fixture: real goal extension/components, no CLI or model calls.
import { appendFileSync } from "node:fs";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import goal from "../../extensions/goal/index.ts";
import type { GoalSnapshot } from "../../extensions/goal/source.ts";

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
  goal(pi, {
    async list() { return { project: snapshot.project, goals: [snapshot.overview.goal] }; },
    async show() { mark("read"); return snapshot; },
    async brief() { return { text: "# Fixture brief\n" + Array.from({ length: 100 }, (_, i) => `Pattern line ${i}: shared ownership, café 👩‍💻`).join("\n") + "\nEND_OF_GOAL_BRIEF" }; },
  });
  pi.on("session_start", () => mark("start"));
  pi.on("agent_start", () => mark("UNEXPECTED_PROVIDER_TURN"));
  pi.on("input", () => ({ action: "handled" }));
  pi.registerCommand("goal-fixture-exit", { description: "Exit isolated fixture", handler: async (_args, ctx) => ctx.shutdown() });
}
