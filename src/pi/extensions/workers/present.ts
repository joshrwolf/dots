import type { MessageRenderer, MessageRenderOptions, Theme } from "@earendil-works/pi-coding-agent";

/** The custom message shape, derived from the public renderer signature. */
type CustomMessage<T> = Parameters<MessageRenderer<T>>[0];
import { VStack, type Component } from "@earendil-works/pi-tui";
import { OutputView, PriorityRow, ToolCard, labelText, type ActivityState, type Notice, type Segment, type Tone } from "../../src/index.ts";

// Presentation for worker events and the live worker widget, composed from
// the companion's presentation library. Pure projection from recorded data;
// the runtime decides what to send, this decides how it reads.

export interface EventDetails {
  id: string;
  kind: string;
  label?: string;
  truncated?: boolean;
  elapsed?: string;
  tokens?: number;
  session?: string;
}

export interface EventModel {
  heading: Segment[];
  status: Segment[];
  notices: Notice[];
  body: string;
  caption: string;
}

const KIND_TONE: Record<string, Tone> = {
  completed: "success",
  failed: "error",
  stopped: "warning",
  asking: "warning",
  message: "muted",
  update: "dim",
  undeliverable: "error",
  paused: "dim",
  relay: "dim",
};

/** The event body after the runtime's one-line header; headers are rebuilt from details. */
function bodyOf(content: CustomMessage<unknown>["content"]): string {
  const text = typeof content === "string"
    ? content
    : content.map((block) => (block.type === "text" ? block.text : "")).filter(Boolean).join("\n");
  const split = text.indexOf("\n\n");
  return split === -1 ? text : text.slice(split + 2);
}

export function projectEvent(message: CustomMessage<EventDetails>): EventModel {
  const details = message.details ?? { id: "?", kind: "event" };
  const kind = labelText(details.kind);
  const heading: Segment[] = [
    { text: labelText(details.label ?? details.id), tone: "toolTitle", priority: 90, shrink: "clip", clip: "end", minWidth: 8 },
    { text: kind, tone: KIND_TONE[kind] ?? "muted", priority: 100, shrink: "clip" },
  ];
  const status: Segment[] = [{ text: labelText(details.id), tone: "dim", priority: 30, shrink: "omit" }];
  if (details.elapsed) status.push({ text: labelText(details.elapsed), tone: "dim", priority: 40, shrink: "omit" });
  if (typeof details.tokens === "number") status.push({ text: `${details.tokens.toLocaleString("en-US")} tok`, tone: "dim", priority: 20, shrink: "omit" });
  const notices: Notice[] = [];
  if (kind === "asking") notices.push({ text: `answer with message { to: "${details.id}", text }`, severity: "warning" });
  if (kind === "undeliverable") notices.push({ text: "message could not be routed; see body", severity: "error" });
  if (details.truncated) notices.push({ text: `excerpt; full text: worker_status { id: "${details.id}" }`, severity: "dim" });
  return { heading, status, notices, body: bodyOf(message.content), caption: details.session ? `session ${details.session}` : "" };
}

/** Collapsed: one row with the first body line as a clipped summary. Expanded: the bounded body. */
export function renderEvent(message: CustomMessage<EventDetails>, options: MessageRenderOptions, theme: Theme): Component {
  const model = projectEvent(message);
  const card = new ToolCard();
  const firstLine = model.body.split("\n").find((line) => line.trim()) ?? "";
  const heading = options.expanded || !firstLine
    ? model.heading
    : [...model.heading, { text: firstLine, tone: "muted" as const, priority: 10, shrink: "clip" as const, clip: "end" as const }];
  const kind = message.details?.kind ?? "";
  const state: ActivityState | undefined = kind === "completed" ? "done" : kind === "failed" || kind === "undeliverable" ? "failed" : kind === "stopped" ? "cancelled" : kind === "asking" ? "queued" : undefined;
  card.update({ heading, status: model.status, notices: model.notices, body: options.expanded ? new OutputView(model.body) : undefined, caption: model.caption, ...(state ? { state } : {}) }, theme);
  const pad = Math.max(0, options.outputPad);
  return {
    render: (width) => card.render(width - 2 * pad).map((line) => " ".repeat(pad) + line),
    invalidate: () => card.invalidate(),
  };
}

/** One line of what a tool call is doing, from its name and arguments. Bounded, sanitized. */
export function describeActivity(toolName: string, args: unknown): string {
  const a = (args && typeof args === "object" ? args : {}) as Record<string, unknown>;
  const str = (key: string) => (typeof a[key] === "string" ? labelText(a[key] as string) : "");
  const first = () => { for (const v of Object.values(a)) if (typeof v === "string" && v) return labelText(v); return ""; };
  const detail = (() => {
    switch (toolName) {
      case "bash": return str("command");
      case "read": case "write": case "edit": case "ls": return str("path");
      case "grep": case "find": return [str("pattern"), str("path")].filter(Boolean).join(" in ");
      case "worker_spawn": return str("label");
      case "message": return `to ${str("to")}`;
      case "worker_status": case "worker_wait": case "worker_stop": return str("id");
      case "codemode": return "script";
      default: return first();
    }
  })();
  return detail ? `${toolName} ${detail.slice(0, 200)}` : toolName;
}

/** The first non-empty line of an assistant message's text, as a glance at what it is saying. */
export function describeText(message: unknown): string {
  const m = message as { role?: string; content?: unknown } | undefined;
  if (!m || m.role !== "assistant" || !Array.isArray(m.content)) return "";
  for (const block of m.content as { type?: string; text?: string }[]) {
    if (block.type === "text" && block.text) {
      const line = block.text.split("\n").find((l) => l.trim());
      if (line) return labelText(line).slice(0, 200);
    }
  }
  return "";
}

export interface WidgetRow {
  label: string;
  state: string;
  /** 0 for workers this session owns; descendants read from the registry nest beneath. */
  depth: number;
  elapsed: string;
  tokens: number;
  calls: number;
  /** Current tool call, or what it last said/did when between calls. */
  detail: string;
  /** Set when the last tool call errored; cleared by the next successful one. */
  failed?: string | undefined;
  /** Milliseconds since the child produced any event. */
  idleMs: number;
}

/** A worker that has produced nothing for this long is shown as quiet (a stall hint, not a verdict). */
export const STALL_MS = 90_000;

function formatMs(ms: number): string {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : s < 3600 ? `${Math.floor(s / 60)}m` : `${Math.floor(s / 3600)}h${Math.floor((s % 3600) / 60)}m`;
}

/** 950 → "950", 10_666 → "10.7k", 1_250_000 → "1.3M". */
export function compact(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

const quiet = (row: WidgetRow) => row.state === "running" && row.idleMs >= STALL_MS;

/** Footer text: the whole tree under this session in a few characters, or empty when idle. */
export function fleetStatus(rows: readonly WidgetRow[]): string {
  const live = rows.filter((r) => r.state === "running" || r.state === "asking");
  if (live.length === 0) return "";
  const asking = live.filter((r) => r.state === "asking").length;
  const failed = live.filter((r) => r.failed).length;
  const stalled = live.filter(quiet).length;
  const parts = [`workers ${live.length}`];
  if (asking) parts.push(`?${asking}`);
  if (failed) parts.push(`\u2717${failed}`);
  if (stalled) parts.push(`quiet ${stalled}`);
  return parts.join(" \u00b7 ");
}

/** Attention first: asking, then a failed last call, then quiet, then oldest. Stable within a tree level. */
export function attentionOrder(a: WidgetRow, b: WidgetRow): number {
  const rank = (r: WidgetRow) => (r.state === "asking" ? 0 : r.failed ? 1 : quiet(r) ? 2 : 3);
  return rank(a) - rank(b);
}

export const DEFAULT_WIDGET_ROWS = 8;

/**
 * Live tree of running/asking workers. Each row: glyph, label (aligned), activity;
 * counters on the right. Descendants are indented under their owner.
 */
export function createWidget(rows: () => WidgetRow[], theme: Theme, maxRows: number = DEFAULT_WIDGET_ROWS): Component {
  const stack = new VStack();
  return {
    render(width) {
      const all = rows();
      if (all.length === 0 || width <= 0) return [];
      stack.clear();
      const shown = all.slice(0, Math.max(1, maxRows));
      const column = Math.min(24, Math.max(...shown.map((r) => r.depth * 2 + labelText(r.label).length)));
      for (const row of shown) {
        const asking = row.state === "asking";
        const stalled = quiet(row);
        const live = row.state === "running" || asking;
        const glyph = !live ? "\u25cb" : asking ? "?" : row.failed ? "\u2717" : "\u25cf";
        const glyphTone: Tone = !live ? "dim" : asking ? "warning" : row.failed ? "error" : stalled ? "dim" : "success";
        const indent = row.depth ? "\u2937 " : "";
        const label = labelText(row.label);
        const padTo = Math.max(label.length, column - row.depth * 2);
        const end: Segment[] = [];
        if (stalled) end.push({ text: `quiet ${formatMs(row.idleMs)}`, tone: "warning", priority: 65, shrink: "omit" });
        end.push(
          { text: row.elapsed, tone: "dim", priority: 60, shrink: "omit" },
          { text: `${row.calls} call${row.calls === 1 ? "" : "s"}`, tone: "dim", priority: 25, shrink: "omit" },
          { text: `${compact(row.tokens)} tok`, tone: "dim", priority: 20, shrink: "omit" },
        );
        const line = new PriorityRow();
        line.update({
          start: [
            { text: `${indent}${glyph}`, tone: glyphTone, priority: 100, shrink: "clip" },
            { text: label, tone: "toolTitle", bold: true, priority: 90, shrink: "clip", clip: "end", minWidth: 8, padTo },
            ...(row.failed ? [{ text: `${row.failed} failed`, tone: "error" as const, priority: 55, shrink: "clip" as const, clip: "end" as const, minWidth: 6 }] : []),
            { text: row.detail, tone: asking ? "warning" : "muted", priority: 50, shrink: "clip", clip: "end", minWidth: 12 },
          ],
          end,
          indent: row.depth > 1 ? (row.depth - 1) * 2 : 0,
          separator: " ",
          endSeparator: " \u00b7 ",
        }, theme);
        stack.addChild(line);
      }
      if (all.length > shown.length) {
        const more = new PriorityRow();
        more.update({ start: [{ text: `+${all.length - shown.length} more \u00b7 /workers`, tone: "dim", priority: 100, shrink: "clip" }], end: [] }, theme);
        stack.addChild(more);
      }
      return stack.render(width);
    },
    invalidate() { stack.invalidate(); },
  };
}
