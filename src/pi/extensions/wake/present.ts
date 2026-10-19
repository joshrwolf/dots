import type { MessageRenderer, MessageRenderOptions, Theme } from "@earendil-works/pi-coding-agent";
import type { Component } from "@earendil-works/pi-tui";
import { OutputView, PreviewView, ActivityCard, activityHeader, labelText, type Notice, type Segment } from "../../src/index.ts";

type CustomMessage<T> = Parameters<MessageRenderer<T>>[0];

export interface WakeDetails {
  id: string;
  fired: number;
  limit: number;
  late: number;
  every?: string;
  manual?: boolean;
  check?: string;
  exit?: number;
  text?: string;
}

function bodyOf(content: CustomMessage<unknown>["content"]): string {
  const text = typeof content === "string"
    ? content
    : content.map((block) => (block.type === "text" ? block.text : "")).filter(Boolean).join("\n");
  const split = text.indexOf("\n\n");
  return split === -1 ? text : text.slice(split + 2);
}

/** Collapsed: wake id, cadence, lateness, and the note. Expanded: the check output too. */
export function renderWake(message: CustomMessage<WakeDetails>, options: MessageRenderOptions, theme: Theme): Component {
  const d = message.details ?? { id: "?", fired: 0, limit: 0, late: 0 };
  const secondary: Segment[] = [{ text: labelText(d.id).slice(0, 12), tone: "dim", priority: 30, shrink: "omit" }];
  const status: Segment[] = [];
  if (d.every) secondary.push({ text: `${d.fired}/${d.limit} · every ${labelText(d.every)}`, tone: "dim", priority: 50, shrink: "omit" });
  if (d.manual) secondary.push({ text: "manual", tone: "dim", priority: 40, shrink: "omit" });
  if (typeof d.exit === "number") status.push({ text: `exit ${d.exit}`, tone: d.exit === 0 ? "muted" : "error", priority: 110, minWidth: 6, shrink: "clip" });
  const notices: Notice[] = [];
  if (d.late > 60_000) notices.push({ text: `late by ${Math.round(d.late / 60_000)}m`, severity: "warning" });
  const body = bodyOf(message.content);
  const card = new ActivityCard();
  card.update({
    header: activityHeader("wake", labelText(d.text ?? "Recorded follow-up"), status, "end", secondary),
    state: typeof d.exit === "number" && d.exit !== 0 ? "failed" : "done",
    notices,
    body: body && (options.expanded || typeof d.exit === "number" && d.exit !== 0 || !d.text) ? options.expanded ? new OutputView(body) : new PreviewView(body, theme) : undefined,
    caption: options.expanded && d.check ? `check: ${d.check}` : "",
  }, theme);
  const pad = Math.max(0, options.outputPad);
  return {
    render: (width) => card.render(width - 2 * pad).map((line) => " ".repeat(pad) + line),
    invalidate: () => card.invalidate(),
  };
}
