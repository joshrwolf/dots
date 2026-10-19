import type { Theme } from "@earendil-works/pi-coding-agent";
import { Spacer, VStack, truncateToWidth, type Component } from "@earendil-works/pi-tui";
import { Panel } from "../../src/ui/panel.ts";
import { PriorityRow, type RowModel, type Tone } from "../../src/ui/row.ts";
import { labelText } from "../../src/ui/text.ts";
import type { GoalView } from "./present.ts";
import type { GoalTask } from "./source.ts";
import { goalTree, groupSummary, isGroup, previewRows, treeRows, type TreeRow, type TreeState } from "./hierarchy.ts";

const text = (value: string, max = 512) => truncateToWidth(labelText(value), max);
const row = (model: RowModel, theme: Theme): PriorityRow => { const view = new PriorityRow(); view.update(model, theme); return view; };
const note = (value: string, theme: Theme, tone: Tone = "dim") => row({ start: [{ text: value, tone, priority: 100, shrink: "clip" }], end: [] }, theme);

export function compactSummary(view: GoalView): string {
  const p = view.snapshot?.overview.progress;
  if (!p || view.error) return view.error ? "unavailable · /goal refresh" : "loading";
  return [p.total ? `${p.done}/${p.total} slices done` : "No slices recorded", p.claimed && `${p.claimed} claimed`, p.held && `${p.held} held`, p.dropped && `${p.dropped} dropped`, view.brief?.warning && "brief needs attention"].filter(Boolean).join(" · ");
}

function progress(view: GoalView, theme: Theme): Component {
  const p = view.snapshot?.overview.progress;
  const filled = p && p.total > 0 ? Math.min(10, Math.floor(10 * p.done / p.total)) : 0;
  return row({ start: [
    { text: compactSummary(view), tone: view.error || view.brief?.warning ? "warning" : "text", priority: 100, shrink: "clip" },
  ], end: p && p.total > 0 && !view.error ? [
    { text: `[${"█".repeat(filled)}${"·".repeat(10 - filled)}]`, tone: p.done === p.total ? "success" : "accent", priority: 0, shrink: "omit" },
  ] : [] }, theme);
}

type TaskState = { label: string; tone: Tone; emphasized?: boolean };
function taskState(task: GoalTask, now: number): TaskState {
  if (task.status === "done") return { label: "✓ Done", tone: "success" };
  if (task.status === "held") return { label: "! Held", tone: "warning" };
  if (task.status === "dropped") return { label: "− Dropped", tone: "dim" };
  if (task.owner && task.lease_until !== null && task.lease_until > now) return { label: "› Claimed", tone: "accent", emphasized: true };
  if (task.blocked_by.length) return { label: "· Waiting", tone: "muted" };
  return { label: "· Open", tone: "muted" };
}

function taskRow(task: GoalTask, view: GoalView, theme: Theme, depth = 0): Component {
  const state = taskState(task, view.snapshot!.now);
  return row({ indent: Math.min(depth, 6) * 2, layout: "inline", start: [
    { text: state.label, tone: state.tone, bold: state.emphasized ?? false, width: 12, minWidth: 1, priority: 100, shrink: "clip" },
    { text: `${depth > 6 ? `[level ${depth + 1}] ` : ""}${text(task.title)}`, tone: state.emphasized ? "accent" : task.status === "done" || task.status === "dropped" ? "muted" : "text", bold: state.emphasized ?? false, minWidth: 8, priority: 80, shrink: "clip" },
  ], end: [] }, theme);
}

function hierarchyRow(item: TreeRow, view: GoalView, theme: Theme, selected?: string): Component {
  const { node, depth, expanded } = item;
  if (!isGroup(node)) return taskRow(node.task, view, theme, depth);
  return row({ indent: Math.min(depth, 6) * 2, layout: "inline", groupSeparator: " · ", start: [
    { text: `${node.task.id === selected ? "› " : ""}${expanded ? "▾" : "▸"}`, tone: "accent", width: 3, priority: 100, shrink: "clip" },
    { text: `${depth > 6 ? `[level ${depth + 1}] ` : ""}${text(node.task.title)}`, tone: "text", bold: true, minWidth: 8, priority: 80, shrink: "clip" },
  ], end: [
    { text: node.task.status === "open" ? "" : node.task.status, tone: node.task.status === "held" ? "warning" : "muted", priority: 95, shrink: "clip" },
    { text: groupSummary(node), tone: node.totals?.held ? "warning" : node.totals?.claimed ? "accent" : "muted", minWidth: 8, priority: 90, shrink: "clip" },
  ] }, theme);
}

function body(view: GoalView, theme: Theme, limit: number, details = false, state?: TreeState): VStack {
  const result = new VStack();
  if (view.error || !view.snapshot) {
    result.addChild(note(view.error ? "Unavailable · /goal refresh" : "Loading goal…", theme, view.error ? "warning" : "muted"));
    if (view.error) result.addChild(note(text(view.error), theme, "warning"));
    return result; // Never decorate a stale snapshot as current progress.
  }
  const { hidden_closed, goal } = view.snapshot.overview;
  result.addChild(progress(view, theme));
  if (goal.root.status !== "open") result.addChild(note(`Goal: ${goal.root.status}`, theme, goal.root.status === "held" ? "warning" : "muted"));
  result.addChild(new Spacer(1));
  const tree = goalTree(view.snapshot);
  const available = treeRows(tree, details ? state?.collapsed : undefined);
  const selected = details ? available.slice(0, limit) : previewRows(tree, limit);
  const grouped = [...tree.nodes.values()].some(isGroup);
  const labels = new Map([...tree.nodes.values()].map(({ task }) => [task.id, text(task.title)]));
  for (const item of selected) {
    const task = item.node.task;
    result.addChild(hierarchyRow(item, view, theme, details ? state?.selected : undefined));
    if (details && task.blocked_by.length) result.addChild(row({ indent: Math.min(item.depth, 6) * 2 + 13, start: [
      { text: `Needs: ${task.blocked_by.slice(0, 8).map((id) => labels.get(id) ?? text(id)).join(", ")}${task.blocked_by.length > 8 ? ` · +${task.blocked_by.length - 8} more` : ""}`, tone: "muted", priority: 100, shrink: "clip" },
    ], end: [] }, theme));
  }
  if (!selected.length) result.addChild(note(hidden_closed ? "No open slices remain." : "No slices recorded.", theme, "muted"));
  const omitted = available.length - selected.length;
  if (omitted > 0) result.addChild(note(`${omitted} more ${details ? "tasks · 3 details for the full-record command" : grouped ? "rows · /goal to inspect" : "slices · /goal to inspect"}`, theme));
  if (tree.warning) result.addChild(note(tree.warning, theme, "warning"));
  if (hidden_closed) result.addChild(note(`${hidden_closed} closed task details hidden · /goal history`, theme));
  if (view.brief?.warning) result.addChild(note(text(view.brief.warning), theme, "warning"));
  return result;
}

/** The persistent panel and inspector share row semantics, not execution state. */
export function goalOverview(view: GoalView, theme: Theme, state?: TreeState): Panel {
  const panel = new Panel();
  panel.update({ heading: { start: [{ text: "Slices", tone: "accent", bold: true, priority: 100, shrink: "clip" }], end: [] },
    body: body(view, theme, 100, true, state),
    footer: { start: [{ text: "Recorded progress · not live execution", tone: "dim", priority: 100, shrink: "clip" }], end: [] },
  }, theme);
  return panel;
}

export class GoalWidget extends VStack {
  override render(width: number): string[] { return width > 0 ? super.render(width) : []; }
  private readonly panel = new Panel();
  constructor() { super(); this.addChild(this.panel); }
  update(view: GoalView, theme: Theme): void {
    this.panel.update({ heading: { layout: "inline", groupSeparator: " · ", start: [
      { text: "Goal", tone: "accent", bold: true, priority: 100, shrink: "clip" },
    ], end: [{ text: text(view.snapshot?.overview.goal.root.title ?? view.focus.id), tone: "text", bold: true, priority: 80, shrink: "clip" }] },
      body: body(view, theme, 4), footer: { start: [{ text: "Recorded progress", tone: "dim", priority: 20, shrink: "omit" }],
        end: [{ text: "inspect · /goal", tone: "muted", priority: 100, shrink: "clip" }] },
    }, theme);
    this.invalidate();
  }
}

/** Selected goal gets detail; other follows remain visible without stacking boxes. */
export class FollowedWidget extends VStack {
  override render(width: number): string[] { return width > 0 ? super.render(width) : []; }
  update(views: readonly GoalView[], selected: string | undefined, theme: Theme): void {
    this.clear();
    if (!views.length) return;
    if (views.length === 1) {
      const widget = new GoalWidget(); widget.update(views[0]!, theme); this.addChild(widget); return;
    }
    const ordered = [...views].sort((a, b) => Number(b.focus.id === selected) - Number(a.focus.id === selected));
    const current = ordered[0]!;
    const content = new VStack();
    content.addChild(note(`${current.focus.id === selected ? ">" : "·"} ${text(current.snapshot?.overview.goal.root.title ?? current.focus.id)}`, theme, "accent"));
    content.addChild(body(current, theme, 2));
    content.addChild(new Spacer(1));
    content.addChild(note("Other followed goals", theme, "muted"));
    for (const view of ordered.slice(1, 6)) content.addChild(row({ layout: "inline", groupSeparator: " · ", start: [
      { text: text(view.snapshot?.overview.goal.root.title ?? view.focus.id), tone: "text", priority: 60, shrink: "clip", minWidth: 10 },
    ], end: [{ text: compactSummary(view), tone: view.error || view.brief?.warning ? "warning" : "muted", priority: 80, shrink: "clip" }] }, theme));
    if (views.length > 6) content.addChild(note(`${views.length - 6} more followed · /goal followed (all remain in context)`, theme));
    const bytes = views.reduce((total, view) => total + new TextEncoder().encode(view.brief?.text ?? "").length, 0);
    const unavailable = views.filter((view) => view.error || view.brief?.warning).length;
    const panel = new Panel();
    panel.update({ heading: { start: [{ text: `Goals · ${views.length} followed${unavailable ? ` · ${unavailable} need attention` : ""}`, tone: unavailable ? "warning" : "accent", bold: true, priority: 100, shrink: "clip" }], end: [] }, body: content,
      footer: { start: [{ text: `~${Math.ceil(bytes / 1024)} KiB brief text`, tone: "dim", priority: 20, shrink: "omit" }], end: [{ text: "inspect · /goal", tone: "muted", priority: 100, shrink: "clip" }] },
    }, theme);
    this.addChild(panel); this.invalidate();
  }
}
