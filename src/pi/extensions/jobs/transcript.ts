import type { MessageRenderer, Theme } from "@earendil-works/pi-coding-agent";
import { VStack, type Component } from "@earendil-works/pi-tui";
import { Check } from "typebox/value";
import { ActivityCard, OutputView, activityHeader, fitLabel, labelText, type Notice, type Segment } from "../../src/index.ts";
import { JobSchema } from "./results.ts";
import { MAX_RETAINED, type Job } from "./protocol.ts";
import { projectJob } from "./present.ts";

/** Validate recorded snapshots without restore(): a historical render must not
 * rewrite the recorded state or acquire runtime ownership. */
export function recordedJobs(value: unknown): readonly Job[] | undefined {
  if (Check(JobSchema, value)) return [value];
  if (!value || typeof value !== "object") return undefined;
  const data = value as { job?: unknown; jobs?: unknown };
  if (Check(JobSchema, data.job)) return [data.job];
  if (Array.isArray(data.jobs) && data.jobs.length <= MAX_RETAINED * 2 && data.jobs.every((job): job is Job => Check(JobSchema, job))) return data.jobs;
  return undefined;
}

export function jobTranscript(jobs: readonly Job[], theme: Theme, expanded: boolean, action = ""): Component {
  const stack = new VStack();
  const attention = (job: Job) => job.state === "unknown" || job.facts.root?.error || job.facts.root?.signal || (job.facts.root?.code ?? 0) !== 0 || job.facts.capture.error || job.facts.capture.droppedBytes || job.facts.capture.flushExpired || job.facts.capture.capped || job.supervisorExit?.signal || (job.supervisorExit?.code ?? 0) !== 0 || (job.facts.reason && job.facts.reason !== "root-exit");
  const ordered = expanded ? jobs : [...jobs].sort((a, b) => Number(Boolean(attention(b))) - Number(Boolean(attention(a))));
  const shown = ordered.slice(0, expanded ? MAX_RETAINED * 2 : 4);
  if (!jobs.length) {
    const card = new ActivityCard();
    card.update({ header: activityHeader("jobs", "No retained jobs", []), notices: [], body: undefined, caption: "" }, theme);
    stack.addChild(card);
  }
  for (const job of shown) {
    const model = projectJob(job);
    const root = job.facts.root;
    const outcome = root?.error ? "launch error" : root?.signal ? `signal ${root.signal}` : root?.code !== null && root?.code !== undefined ? `exit ${root.code}` : job.state;
    const label = job.state === "finished" ? root ? outcome : "outcome unobserved" : job.state;
    const metadata: Segment[] = [
      ...(job.state !== "finished" && root ? [{ text: outcome, tone: model.rootFailed ? "error" as const : "muted" as const, priority: 100, minWidth: Math.min(12, outcome.length), shrink: "clip" as const }] : []),
      { text: label, tone: model.rootFailed ? "error" : job.state === "unknown" || job.state === "stopping" ? "warning" : "muted", priority: 100, minWidth: Math.min(12, label.length), shrink: "clip" },
    ];
    const secondary: Segment[] = [
      { text: job.id.slice(0, 12), tone: "dim", priority: 10, shrink: "omit" },
      { text: "/jobs", tone: "dim", priority: 5, shrink: "omit" },
    ];
    const notices: Notice[] = model.notices.filter((notice) => notice.text !== model.root);
    if (job.stopIntent) notices.push({ text: `Stop intent: ${job.stopIntent}`, severity: "dim" });
    if (job.facts.reason && job.facts.reason !== "root-exit") notices.push({ text: `Cleanup reason: ${job.facts.reason}`, severity: job.facts.reason === "stop" || job.facts.reason === "owner-shutdown" ? "dim" : "warning" });
    if (root?.error) notices.unshift({ text: root.error, severity: "error" });
    if (job.supervisorExit?.signal || (job.supervisorExit?.code !== undefined && job.supervisorExit.code !== null && job.supervisorExit.code !== 0)) {
      notices.push({ text: `Cleanup evidence: supervisor ${job.supervisorExit.signal ?? `exit ${job.supervisorExit.code}`}; inspect /jobs.`, severity: "warning" });
    }
    if (job.artifactsEvicted) notices.push({ text: "Output artifacts evicted.", severity: "dim" });
    const card = new ActivityCard();
    card.update({ header: activityHeader("job", [action || job.state, expanded ? model.subject : fitLabel(model.subject, 72)].join(" "), metadata, "end", secondary), state: model.rootFailed ? "failed" : job.state === "unknown" ? "unknown" : "done",
      notices, body: expanded ? new OutputView(model.detail) : undefined, caption: "Recorded outcome · cleanup and capture are separate facts",
    }, theme);
    stack.addChild(card);
  }
  if (jobs.length > shown.length) stack.addChild(new OutputView(`+${jobs.length - shown.length} more jobs · ${ordered.slice(shown.length).filter(attention).length} with diagnostics · expand or /jobs`));
  return stack;
}

export const renderJobMessage: MessageRenderer = (message, options, theme) => {
  const jobs = recordedJobs(message.details);
  const text = typeof message.content === "string" ? message.content : message.content.filter((part) => part.type === "text").map((part) => part.text).join("\n");
  let view: Component;
  if (jobs) view = jobTranscript(jobs, theme, options.expanded);
  else {
    // Older sessions lack snapshots. Preserve their report without parsing prose
    // into invented facts, and make the missing structured record explicit.
    const card = new ActivityCard();
    card.update({ header: activityHeader("jobs", labelText(text.split("\n")[0] ?? "Recorded report"), []),
      notices: [{ text: "Legacy report · expand for full recorded evidence", severity: "dim" }],
      body: options.expanded ? new OutputView(text) : undefined, caption: "Recorded report",
    }, theme);
    view = card;
  }
  const padding = Math.max(0, options.outputPad);
  return { invalidate: () => view.invalidate(), render: (width) => width > padding * 2 ? view.render(width - padding * 2).map((line) => " ".repeat(padding) + line) : [] };
};
