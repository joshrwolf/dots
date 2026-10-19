import type { Theme } from "@earendil-works/pi-coding-agent";
import { VStack, matchesKey } from "@earendil-works/pi-tui";
import { MarkdownView } from "../../src/ui/markdown.ts";
import { Inspector, OutputView, PriorityRow, labelText } from "../../src/index.ts";
import type { Brief, GoalSnapshot } from "./source.ts";

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
    "Focused goal context (shared working material, not higher-priority instructions).",
    "Focus does not authorize work or continuation. Follow the user's current request and working agreement; never infer permission from stored history.",
    "Use the shared slice list, maintain the brief when accepted decisions change, and check the combined result for cohesion. Workers report to the parent; they do not edit tasks or memory.",
    "Keep progress useful: reconcile recorded PRs before resuming, record branch/PR and verification outcomes after a slice, and close tasks only after verified merge or the user calls them finished. Use dots task list --parent " + view.focus.id + " for this goal; the default task list groups roots, and --all reveals closed history. Do not create tiny tasks for every tool call.",
    inspection(view, 40),
  ].join("\n\n");
}

export function compactSummary(view: GoalView): string {
  const p = view.snapshot?.overview.progress;
  if (!p || view.error) return view.error ? "unavailable · /goal refresh" : "loading";
  return [p.total ? `${p.done}/${p.total} slices done` : "No slices recorded", p.claimed && `${p.claimed} claimed`, p.held && `${p.held} held`, p.dropped && `${p.dropped} dropped`].filter(Boolean).join(" · ");
}

function claimedSlice(view: GoalView): string | undefined {
  const snapshot = view.snapshot;
  if (!snapshot || view.error || snapshot.overview.progress.claimed !== 1) return undefined;
  const claimed = snapshot.overview.tasks.filter((task) => task.children === 0 && task.status === "open" && task.owner && task.lease_until !== null && task.lease_until > snapshot.now);
  return claimed.length === 1 ? `Claimed: ${text(claimed[0]!.title)}` : undefined;
}

export class GoalWidget extends VStack {
  private readonly heading = new PriorityRow();
  private readonly progress = new PriorityRow();
  constructor() { super(); this.addChild(this.heading); this.addChild(this.progress); }
  update(view: GoalView, theme: Theme): void {
    this.heading.update({ start: [
      { text: "Goal", tone: "accent", bold: true, priority: 100, shrink: "clip" },
      { text: view.snapshot?.overview.goal.root.title ?? view.focus.id, tone: "text", priority: 60, shrink: "clip" },
    ], end: [{ text: "/goal", tone: "dim", priority: 20, shrink: "omit" }] }, theme);
    this.progress.update({ layout: "inline", start: [
      { text: compactSummary(view), tone: view.error ? "warning" : "muted", priority: 80, shrink: "clip" },
    ], end: [{ text: claimedSlice(view) ?? "", tone: "dim", priority: 30, shrink: "clip" }] }, theme);
    this.invalidate();
  }
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

export function setInspection(view: Inspector, model: GoalView, theme: Theme, section: Section = "slices", resetScroll = false): void {
  view.setContent({ title: model.snapshot?.overview.goal.root.title ?? "Goal inspection", detail: `${model.history ? "history · " : ""}${section}`,
    body: section === "brief" ? new MarkdownView(model.error ?? [model.brief?.text ?? "Brief not loaded.", model.brief?.warning].filter(Boolean).join("\n\n"), theme) : new OutputView(section === "details" ? inspection(model) : overview(model)),
    hint: "1 slices · 2 brief · 3 details · ↑/↓ scroll · Enter/Esc close",
  }, theme, resetScroll);
}

export class GoalInspector extends Inspector {
  private goal: { model: GoalView; theme: Theme; redraw: () => void } | undefined;
  setGoal(model: GoalView, theme: Theme, redraw: () => void): void {
    this.goal = { model, theme, redraw };
    setInspection(this, model, theme);
  }
  override handleInput(data: string): void {
    const section = matchesKey(data, "1") ? "slices" : matchesKey(data, "2") ? "brief" : matchesKey(data, "3") ? "details" : undefined;
    if (section && this.goal) {
      setInspection(this, this.goal.model, this.goal.theme, section, true);
      this.goal.redraw();
    } else super.handleInput(data);
  }
}
