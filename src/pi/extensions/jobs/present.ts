import type { Theme } from "@earendil-works/pi-coding-agent";
import { VStack, type Component } from "@earendil-works/pi-tui";
import { OutputView, PriorityRow, ToolCard, labelText, sourceText, type Notice, type Segment } from "../../src/index.ts";
import { live, type Job } from "./protocol.ts";

export interface JobView {
  readonly id: string;
  readonly subject: string;
  readonly state: Job["state"];
  readonly live: boolean;
  readonly stoppable: boolean;
  readonly root: string;
  readonly rootFailed: boolean;
  readonly cleanup: string;
  readonly notices: readonly Readonly<Notice>[];
  readonly detail: string;
}

/** Only currently observed admitted/running jobs can offer a human stop action.
 * Restored unfinished jobs are unknown; neither a PID nor a historical id grants control.
 */
export function canStop(job: Job): boolean {
  return (job.state === "accepted" || job.state === "running") && !job.observationLoss && !job.stopIntent && !job.facts.reason;
}

/** Copy bounded display values once. Rendering never retains mutable runtime records. */
export function projectJob(job: Job): JobView {
  const outcome = job.facts.root;
  const root = outcome?.error ? `root launch error: ${outcome.error.slice(0, 512)}`
    : outcome?.signal ? `root signal ${outcome.signal}`
      : outcome?.code !== null && outcome?.code !== undefined ? `root exit ${outcome.code}` : "root outcome unobserved";
  const rootFailed = Boolean(outcome && (outcome.error || outcome.signal || (outcome.code !== null && outcome.code !== 0)));
  const cleanup = job.state === "unknown" ? "cleanup unknown"
    : job.state === "stopping" ? "cleanup in progress"
      : job.supervisorExit ? `supervisor ${job.supervisorExit.signal ?? `exit ${job.supervisorExit.code}`}; ${job.facts.killIntent ? "KILL intent recorded" : "no KILL intent"}`
        : "cleanup not yet observed";
  const c = job.facts.capture;
  const notices: Notice[] = [];
  if (job.observationLoss || job.state === "unknown") notices.push({ severity: "warning", text: job.observationLoss ?? "Historical/unknown job: process control is unavailable." });
  if (rootFailed) notices.push({ severity: "error", text: root });
  if (c.capped) notices.push({ severity: "warning", text: "Output exceeded the 8 MiB prefix cap; inspect the recent tail too." });
  if (c.droppedBytes || c.error || c.flushExpired) notices.push({ severity: "warning", text: `Capture incomplete: ${c.droppedBytes} bytes dropped${c.flushExpired ? "; flush deadline expired" : ""}${c.error ? `; ${c.error}` : ""}.` });
  if (!live(job) && (!c.prefixFlushed || !c.tailFlushed || !c.streamsEnded)) notices.push({ severity: "warning", text: "Capture is not confirmed complete: streams or final prefix/tail flush are unobserved." });
  const detail = [
    `Job: ${job.id}`,
    `State: ${job.state}`,
    `Root: ${root.replace(/^root /, "")}`,
    `Cleanup: ${cleanup.replace(/^cleanup /, "")}`,
    `Stop intent: ${job.stopIntent ?? "none"}; cleanup reason: ${job.facts.reason ?? "none"}`,
    `TERM: ${job.facts.termSent ? "sent" : "unobserved"}; group KILL: ${job.facts.killIntent ? "intent recorded" : "unobserved"}`,
    "Cleanup evidence is not universal process containment.",
    job.artifactsEvicted ? "Output artifacts evicted." : `Output prefix: ${job.directory}/output.log\nRecent tail: ${job.directory}/tail.log`,
    `Capture: ${c.bytesSeen} bytes seen; ${c.prefixBytes} prefix bytes written; ${c.tailBytes} tail bytes; streams ended=${c.streamsEnded}; prefix flushed=${c.prefixFlushed}; tail flushed=${c.tailFlushed}`,
    `Cwd: ${job.cwd}`,
    `Command:\n${job.command}`,
  ].join("\n");
  return Object.freeze({
    id: job.id, subject: labelText(job.command.slice(0, 256)) + (job.command.length > 256 ? "…" : ""),
    state: job.state, live: live(job), stoppable: canStop(job), root: labelText(root), rootFailed,
    cleanup: labelText(cleanup), notices: Object.freeze(notices.map((n) => Object.freeze({ ...n, text: sourceText(n.text) }))), detail: sourceText(detail),
  });
}

function heading(model: JobView): Segment[] {
  return [
    { text: "job", tone: "toolTitle", bold: true, priority: 90, shrink: "clip" },
    { text: model.subject, tone: "text", priority: 60, shrink: "clip", minWidth: 8 },
  ];
}
function status(model: JobView): Segment[] {
  return [
    { text: model.state, tone: model.state === "unknown" ? "warning" : "muted", priority: 110, shrink: "clip" },
    { text: model.root, tone: model.rootFailed ? "error" : "muted", priority: 100, shrink: "clip" },
    { text: model.id.slice(0, 10), tone: "dim", priority: 10, shrink: "omit" },
  ];
}

/** One compact keyed widget, at most four rows plus its header; no row timers. */
export class JobsWidget implements Component {
  private readonly stack = new VStack();
  update(models: readonly JobView[], theme: Theme): void {
    const visible = models.filter((m) => m.live || m.state === "unknown");
    const header = new PriorityRow();
    header.update({ layout: "inline", start: [{ text: "jobs", tone: "toolTitle", bold: true, priority: 80, shrink: "clip" }], end: [
      { text: `${visible.filter((m) => m.live).length} active · ${visible.filter((m) => m.state === "unknown").length} unknown`, tone: "muted", priority: 90, shrink: "clip" },
      { text: "/jobs", tone: "dim", priority: 20, shrink: "omit" },
    ] }, theme);
    this.stack.clear();
    this.stack.addChild(header);
    for (const model of visible.slice(0, 4)) {
      const row = new PriorityRow();
      row.update({ layout: "inline", start: heading(model), end: status(model), endSeparator: " · " }, theme);
      this.stack.addChild(row);
    }
    if (visible.length > 4) {
      const more = new PriorityRow();
      more.update({ start: [{ text: `+${visible.length - 4} more · /jobs`, tone: "dim", priority: 100, shrink: "clip" }], end: [] }, theme);
      this.stack.addChild(more);
    }
  }
  invalidate(): void { this.stack.invalidate(); }
  render(width: number): string[] { return width > 0 ? this.stack.render(width) : []; }
}

export function inspectCard(model: JobView, theme: Theme): ToolCard {
  const card = new ToolCard();
  card.update({
    heading: heading(model), status: status(model), notices: model.notices, noticeIndent: 2,
    body: new OutputView(model.detail), caption: "Recorded snapshot · output paths are for ordinary read",
  }, theme);
  return card;
}
