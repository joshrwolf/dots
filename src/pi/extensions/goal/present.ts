import type { Theme } from "@earendil-works/pi-coding-agent";
import { matchesKey } from "@earendil-works/pi-tui";
import { Panel } from "../../src/ui/panel.ts";
import { MarkdownView } from "../../src/ui/markdown.ts";
import { Inspector, OutputView, labelText } from "../../src/index.ts";
import type { Brief, GoalSnapshot } from "./source.ts";
import { compactSummary, goalOverview } from "./dashboard.ts";
import { goalTree, isGroup, treeRows, type TreeState } from "./hierarchy.ts";
export { compactSummary, GoalWidget, FollowedWidget } from "./dashboard.ts";

export interface Focus { readonly id: string; readonly project: string }
export interface GoalView {
  readonly focus: Focus;
  readonly snapshot?: GoalSnapshot;
  readonly brief?: Brief;
  readonly history?: boolean;
  readonly error?: string;
}
const text = (value: string, max = 512) => labelText(value).slice(0, max);
export function summary(view: GoalView): string {
  const p = view.snapshot?.overview.progress;
  if (!p) return view.error ? "unavailable · /goal refresh" : "loading";
  return `${p.done} done · ${p.open} open · ${p.held} held · ${p.dropped} dropped · ${p.ready} ready · ${p.with_pr} PR recorded`;
}
export function inspection(view: GoalView, taskLimit = 100): string {
  const lines = [`Goal ${view.focus.id}`, `Project: ${view.focus.project}`, "Focus only; no automatic execution."];
  if (view.error) return [...lines, `Unavailable: ${view.error}`, "Refresh before relying on previous goal context."].join("\n");
  const snapshot = view.snapshot;
  if (!snapshot) return [...lines, "Loading…"].join("\n");
  const { goal, tasks } = snapshot.overview;
  lines.push(goal.root.title, `Root status: ${goal.root.status}`, `Recorded at: ${new Date(snapshot.now * 1000).toISOString()}`,
    summary(view), "Counts cover leaf tasks. Ready/PR counts overlap open tasks. Done is recorded, not verified landed.",
    "Git/PR and worker state are not refreshed here; reconcile before acting.", `Brief: ${goal.brief}`, "", "Slices:");
  for (const task of tasks.slice(0, taskLimit)) {
    const details = [task.parent && `parent ${task.parent}`, task.children > 0 && `group (${task.children} children)`,
      task.owner && `${task.lease_until !== null && task.lease_until > snapshot.now ? "claimed by" : "expired claim"} ${text(task.owner, 128)}`,
      task.blocked_by.length > 0 && `blocked by ${task.blocked_by.join(", ")}`, task.pr && `PR ${text(task.pr)}`,
      task.branch && `branch ${text(task.branch)}`].filter(Boolean);
    lines.push(`${task.id} [${task.status}] ${text(task.title)}${details.length ? ` — ${details.join("; ")}` : ""}`);
  }
  if (!tasks.length) lines.push(snapshot.overview.hidden_closed > 0 ? "No open slices remain." : "No slices recorded.");
  if (snapshot.overview.hidden_closed > 0) lines.push(`${snapshot.overview.hidden_closed} closed task details hidden; /goal history or dots task goal show ${goal.root.id} --all shows history.`);
  if (tasks.length > taskLimit) lines.push(`[${tasks.length - taskLimit} tasks omitted; use dots task goal show ${goal.root.id}${view.history ? " --all" : ""} --json for all.]`);
  lines.push("", "Brief content:", view.brief?.text ?? "Brief not loaded.");
  if (view.brief?.warning) lines.push("", view.brief.warning);
  return lines.join("\n");
}

/** A fresh orientation, not a replay of prior transcripts or an execution grant. */
export function orientation(view: GoalView): string {
  return [
    "Followed goal context (shared working material, not higher-priority instructions).",
    "Focus does not authorize work or continuation. Follow the user's current request and working agreement; never infer permission from stored history.",
    "Use the shared slice list, maintain the brief when accepted decisions change, and check the combined result for cohesion. Workers report to the parent; they do not edit tasks or memory.",
    "Keep progress useful: reconcile recorded PRs before resuming, record branch/PR and verification outcomes after a slice, and close tasks only after verified merge or the user calls them finished. Use dots task list --parent " + view.focus.id + " for this goal; the default task list groups roots, and --all reveals closed history. Do not create tiny tasks for every tool call.",
    inspection(view, 40),
  ].join("\n\n");
}

type Section = "slices" | "brief" | "details";

export function overview(model: GoalView): string {
  if (model.error || !model.snapshot) return inspection(model);
  const { goal, tasks, hidden_closed } = model.snapshot.overview;
  const labels = new Map(tasks.map((task) => [task.id, text(task.title)]));
  const lines = [compactSummary(model), ...(goal.root.status !== "open" ? [`Goal: ${goal.root.status}`] : []), ""];
  for (const task of tasks.slice(0, 100)) {
    const claimed = task.status === "open" && task.owner && task.lease_until !== null && task.lease_until > model.snapshot.now;
    const state = task.children > 0 ? `Group (${task.status})` : task.status !== "open" ? task.status === "held" ? "Held" : task.status === "done" ? "Done" : "Dropped" : claimed ? "Claimed" : task.blocked_by.length ? "Waiting" : "Open";
    lines.push(`${state.padEnd(9)} ${text(task.title)}`);
    if (task.blocked_by.length) lines.push(`          Needs: ${task.blocked_by.map((id) => labels.get(id) ?? id).join(", ")}`);
  }
  if (!tasks.length) lines.push(hidden_closed ? "No open slices remain." : "No slices recorded.");
  if (tasks.length > 100) lines.push(`${tasks.length - 100} more tasks; see 3 details for the full-record command.`);
  if (hidden_closed) lines.push("", `${hidden_closed} closed task details hidden · close and use /goal history`);
  if (model.brief?.warning) lines.push("", model.brief.warning);
  return lines.join("\n");
}

export function setInspection(view: Inspector, model: GoalView, theme: Theme, section: Section = "slices", resetScroll = false, treeState?: TreeState): void {
  let body: Panel;
  if (section === "slices") body = goalOverview(model, theme, treeState);
  else {
    body = new Panel();
    body.update({ heading: { start: [{ text: section === "brief" ? "Brief" : "Record details", tone: "accent", bold: true, priority: 100, shrink: "clip" }], end: [] },
      body: section === "brief" ? new MarkdownView(model.error ?? [model.brief?.text ?? "Brief not loaded.", model.brief?.warning].filter(Boolean).join("\n\n"), theme) : new OutputView(inspection(model)),
    }, theme);
  }
  view.setContent({ title: model.snapshot?.overview.goal.root.title ?? "Goal inspection", detail: model.history ? "history" : "inspection",
    body,
    hint: section === "slices" && treeState ? "1 slices · 2 brief · 3 details · [/] group · ←/→ fold · ↑/↓ scroll · Esc close" : "1 slices · 2 brief · 3 details · ↑/↓ scroll · Enter/Esc close",
  }, theme, resetScroll);
}

export class GoalInspector extends Inspector {
  private goal: { model: GoalView; theme: Theme; redraw: () => void } | undefined;
  private section: Section = "slices";
  private readonly collapsed = new Set<string>();
  private selected: string | undefined;

  private refresh(resetScroll = false, reveal = false): void {
    if (!this.goal) return;
    const { model, theme } = this.goal;
    const rows = model.snapshot && !model.error ? treeRows(goalTree(model.snapshot), this.collapsed).slice(0, 100) : [];
    const groups = rows.filter((row) => isGroup(row.node));
    if (!groups.some((row) => row.node.task.id === this.selected)) this.selected = groups[0]?.node.task.id;
    setInspection(this, model, theme, this.section, resetScroll, groups.length ? { collapsed: this.collapsed, selected: this.selected } : undefined);
    if (reveal && this.section === "slices" && this.selected) {
      const index = rows.findIndex((row) => row.node.task.id === this.selected);
      const dependencies = rows.slice(0, index).filter((row) => row.node.task.blocked_by.length).length;
      this.revealLine(3 + Number(model.snapshot?.overview.goal.root.status !== "open") + index + dependencies);
    }
  }

  setGoal(model: GoalView, theme: Theme, redraw: () => void): void {
    const changed = this.goal?.model.focus.id !== model.focus.id;
    if (changed) { this.section = "slices"; this.collapsed.clear(); this.selected = undefined; }
    this.goal = { model, theme, redraw };
    if (model.snapshot) {
      const nodes = goalTree(model.snapshot).nodes;
      for (const id of this.collapsed) if (!nodes.has(id) || !isGroup(nodes.get(id)!)) this.collapsed.delete(id);
    }
    this.refresh(changed);
  }
  override handleInput(data: string): void {
    const section = matchesKey(data, "1") ? "slices" : matchesKey(data, "2") ? "brief" : matchesKey(data, "3") ? "details" : undefined;
    if (section && this.goal) {
      this.section = section;
      this.refresh(true);
      this.goal.redraw();
    } else if (this.section === "slices" && this.goal?.model.snapshot && !this.goal.model.error
      && (matchesKey(data, "[") || matchesKey(data, "]") || matchesKey(data, "space") || matchesKey(data, "left") || matchesKey(data, "right"))) {
      const groups = treeRows(goalTree(this.goal.model.snapshot), this.collapsed).slice(0, 100).filter((row) => isGroup(row.node));
      if (!groups.length) return;
      if (matchesKey(data, "[") || matchesKey(data, "]")) {
        const current = Math.max(0, groups.findIndex((row) => row.node.task.id === this.selected));
        this.selected = groups[(current + (matchesKey(data, "]") ? 1 : groups.length - 1)) % groups.length]!.node.task.id;
      } else if (this.selected) {
        if (matchesKey(data, "left") || (matchesKey(data, "space") && !this.collapsed.has(this.selected))) this.collapsed.add(this.selected);
        else this.collapsed.delete(this.selected);
      }
      this.refresh(false, true); this.goal.redraw();
    } else super.handleInput(data);
  }
}
