import type { Theme } from "@earendil-works/pi-coding-agent";
import { VStack, type Component } from "@earendil-works/pi-tui";
import type { PresentationView, ToolObservation, ToolPresentation } from "../../host/tools.ts";
import { ActivityCard, OutputView, PreviewView, activityHeader, type Notice } from "../../ui/card.ts";
import { labelText } from "../../ui/text.ts";
import { fitLabel, PriorityRow } from "../../ui/row.ts";
import type { ActivityState } from "../../ui/state.ts";
import { jobTranscript, recordedJobs } from "../../../extensions/jobs/transcript.ts";
import { activityState, duration, outcome, positive, record, segment, string, type Outcome } from "./records.ts";

export const backgroundToolNames = ["job_start", "job_status", "job_stop", "worker_spawn", "worker_status", "worker_wait", "worker_stop", "message", "wake", "wake_list", "wake_cancel", "wake_run"] as const;
const workerNames: readonly string[] = ["worker_spawn", "worker_status", "worker_wait", "worker_stop", "message"];
const wakeNames: readonly string[] = ["wake", "wake_list", "wake_cancel", "wake_run"];
const verbs: Record<string, string> = { job_start: "start", job_status: "inspect", job_stop: "stop", worker_spawn: "spawn", worker_status: "inspect", worker_wait: "wait", worker_stop: "stop", message: "message", wake: "schedule", wake_list: "list", wake_cancel: "cancel", wake_run: "run" };

interface ActivityModel { readonly name: string; readonly subject: string; readonly details: unknown; readonly outcome: Outcome }
function subject(args: unknown): string {
  const a = record(args) ?? {};
  for (const key of ["label", "command", "path", "pattern", "query", "url", "id", "to", "text"]) {
    const value = string(a[key]);
    if (value) return value;
  }
  return a.all === true ? "all" : "";
}
function records(value: unknown): readonly Record<string, unknown>[] {
  const entries = Array.isArray(value) ? value : [value];
  return entries.slice(0, 32).flatMap((entry) => { const item = record(entry); return item ? [item] : []; });
}
const firstLine = (value: string) => value.split("\n").find((line) => line.trim()) ?? "";

class ActivityView implements PresentationView<ActivityModel> {
  private view: Component = new VStack();
  update(model: ActivityModel, theme: Theme, expanded: boolean): void {
    const result = model.outcome;
    if (result.kind === "result" && !result.error && !result.partial && model.name.startsWith("job_")) {
      const jobs = recordedJobs(model.details);
      if (jobs) { this.view = jobTranscript(jobs, theme, expanded, verbs[model.name]); return; }
    }
    const worker = workerNames.includes(model.name);
    const wake = wakeNames.includes(model.name);
    const kind = worker ? "worker" : wake ? "wake" : model.name.startsWith("job_") ? "job" : model.name;
    let title = [verbs[model.name], model.subject].filter(Boolean).join(" ");
    const notices: Notice[] = [];
    const status = duration(result, expanded);
    let state: ActivityState = activityState(result);
    let body: Component | undefined;
    if (result.kind === "result") {
      // Text remains authoritative for unknown/legacy/mixed results. Do not
      // manufacture success from JSON fields or silently discard diagnostics.
      body = result.text ? new OutputView(result.text) : undefined;
      const truncation = record(record(model.details)?.truncation);
      if (truncation?.truncated === true) notices.push({ text: "Recorded result truncated", severity: "warning" });
      const fullOutputPath = string(record(model.details)?.fullOutputPath);
      if (fullOutputPath) notices.push({ text: `Full output: ${fullOutputPath}`, severity: "dim" });
      if (result.image) status.push(segment(result.showImages ? "image" : "image · preview hidden"));
      if (!result.partial && !result.error) {
        const items = records(model.details);
        const one = items.length === 1 ? items[0] : undefined;
        if (worker && one && typeof one.label === "string" && typeof one.state === "string" && typeof one.processState === "string" && typeof one.quiet === "boolean") {
          title = [verbs[model.name], one.label].filter(Boolean).join(" ");
          status.push(segment(one.state, one.state === "failed" ? "error" : one.state === "asking" ? "warning" : "muted", 100));
          if (one.state === "failed") state = "failed";
          if (one.state === "asking") state = "waiting";
          if (one.quiet) notices.push({ text: `No recent events · ${string(one.quietFor) ?? "quiet"}`, severity: "warning" });
          const children = record(one.children);
          for (const key of ["asking", "failed", "quiet"]) {
            const count = positive(children?.[key]);
            if (count) notices.push({ text: `${count} descendant${count === 1 ? "" : "s"} ${key} · expand to inspect`, severity: "warning" });
          }
          if (Array.isArray(one.pendingQuestions) && one.pendingQuestions.length > 1) {
            for (const question of one.pendingQuestions.slice(0, 8)) if (typeof question === "string" && question !== one.question) notices.push({ text: question, severity: "warning" });
            if (one.pendingQuestions.length > 8) notices.push({ text: `${one.pendingQuestions.length - 8} more pending questions`, severity: "warning" });
          }
          if (typeof one.question === "string" && one.question) notices.push({ text: one.question, severity: "warning" });
          if (typeof one.error === "string" && one.error) notices.push({ text: one.error, severity: "error" });
          if (typeof one.lastFailure === "string" && one.lastFailure) notices.push({ text: `Last call: ${one.lastFailure}`, severity: "warning" });
          if (expanded || one.processState === "stopping" || (one.state === "running" && one.processState !== "alive")) status.push(segment(`process ${one.processState}`, "dim", 30));
          if (!expanded && result.text && !result.image) {
            const preview = model.name === "worker_status" ? string(one.state === "completed" ? one.result : one.activity) ?? "" : result.text;
            body = preview ? new PreviewView(preview, theme) : undefined;
          }
          // Spawn/status acknowledgements are redundant with the facts above;
          // messages and wait results contain useful replies, so keep a preview.
          if (!expanded && model.name === "worker_spawn") body = undefined;
          if (!expanded && !result.image && model.name === "worker_status" && one.state === "completed" && typeof one.result === "string" && one.result) {
            body = undefined;
            notices.push({ text: "Result available · Ctrl+O to inspect report and transcript", severity: "dim" });
          }
        } else if (worker && record(model.details)?.answered === false) {
          status.push(segment("no answer", "warning", 100));
        }
        if (wake && one && typeof one.text === "string" && typeof one.active === "boolean") {
          title = [verbs[model.name], one.text].filter(Boolean).join(" ");
          status.push(segment(one.active ? typeof one.in === "string" ? `in ${one.in}` : "scheduled" : "inactive", "muted", 100));
          if (!expanded && (model.name === "wake" || model.name === "wake_cancel")) body = undefined;
        }
        if (Array.isArray(model.details)) {
          status.push(segment(`${model.details.length} ${worker ? "workers" : wake ? "wakes" : "records"}`));
          const recognized = model.details.length <= 32 && items.length === model.details.length &&
            items.every((item) => worker ? typeof item.label === "string" && typeof item.state === "string" && typeof item.processState === "string" && typeof item.quiet === "boolean" : wake ? typeof item.text === "string" && typeof item.active === "boolean" : false);
          if (recognized && (worker || wake)) {
            // Lists summarize recorded facts, not the first worker's prose.
            // Surface every question/failure even when its row is folded away.
            for (const item of items) {
              const label = string(item.label) ?? string(item.text) ?? "";
              if (item.state === "failed") { state = "failed"; notices.push({ text: `${label}: ${string(item.error) ?? "failed"}`, severity: "error" }); }
              if (typeof item.question === "string" && item.question) notices.push({ text: `${label}: ${item.question}`, severity: "warning" });
              if (item.state === "asking" && state !== "failed") state = "waiting";
              if (item.quiet) notices.push({ text: `${label}: no recent events · ${string(item.quietFor) ?? "quiet"}`, severity: "warning" });
              const children = record(item.children);
              for (const key of ["asking", "failed", "quiet"]) {
                const count = positive(children?.[key]);
                if (count) notices.push({ text: `${label}: ${count} descendant${count === 1 ? "" : "s"} ${key}`, severity: "warning" });
              }
              if (typeof item.lastFailure === "string" && item.lastFailure) notices.push({ text: `${label}: last call ${item.lastFailure}`, severity: "warning" });
              if (typeof item.lastCode === "number" && item.lastCode !== 0) notices.push({ text: `${label}: last check exit ${item.lastCode}`, severity: "warning" });
            }
            if (!expanded && !result.image) {
              const rows = new VStack();
              for (const item of items.slice(0, 8)) {
                const row = new PriorityRow();
                const label = string(item.label) ?? string(item.text) ?? "";
                const fact = worker ? string(item.state) ?? "unknown" : item.active ? `in ${string(item.in) ?? "unknown"}` : "inactive";
                row.update({ layout: "inline", start: [segment(label, "text", 40)], end: [segment(fact, item.state === "failed" ? "error" : item.state === "asking" ? "warning" : "muted", 100)] }, theme);
                rows.addChild(row);
              }
              if (items.length > 8) rows.addChild(new OutputView(`+${items.length - 8} more · Ctrl+O to expand`));
              body = items.length ? rows : undefined;
            }
          }
        }
      }
      if (result.error) status.push(segment("failed", "error", 100));
      if (result.text && !body && expanded) body = new OutputView(result.text);
      if (!result.text && !result.image && !result.partial) notices.push({ text: "No text result recorded", severity: "dim" });
    }
    const card = new ActivityCard();
    card.update({ header: activityHeader(kind, expanded ? title : fitLabel(labelText(title), 80), status, "end"), state, notices, body,
      caption: expanded && model.subject ? firstLine(model.subject) : "", captionPolicy: "when-not-in-header" }, theme);
    this.view = card;
  }
  invalidate(): void { this.view.invalidate(); }
  render(width: number): string[] { return width > 0 ? this.view.render(width) : []; }
}

/** Shared transcript shell, including conservative fallback for otherwise
 * unstyled tools. Recorded text stays available; images remain host-owned. */
export function activityPresentation(name: string): ToolPresentation<ActivityModel> {
  return { shell: "self", project: (frame: ToolObservation) => ({ name, subject: subject(frame.args), details: frame.slot === "result" ? frame.details : undefined, outcome: outcome(frame) }), create: () => new ActivityView() };
}
