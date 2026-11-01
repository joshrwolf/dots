import type {EventDetails, EventKind} from './record.ts';
import type {TaskAssignment} from './assignment.ts';
import type {
  MessageRenderer,
  MessageRenderOptions,
  Theme,
} from '@earendil-works/pi-coding-agent';

/** The custom message shape, derived from the public renderer signature. */
type CustomMessage<T> = Parameters<MessageRenderer<T>>[0];
import {VStack, visibleWidth, type Component} from '@earendil-works/pi-tui';
import {
  activityDecoration,
  activityHeader,
  PriorityRow,
  ActivityCard,
  contextMeter,
  labelText,
  type ActivityState,
  type Notice,
  type Segment,
  type Tone,
} from '../../src/index.ts';

import {
  ActivitySection,
  activityHeading,
  activityRows,
} from '../../src/ui/activity-section.ts';
import {MarkdownView} from '../../src/ui/markdown.ts';
import {workerFrame} from '../../src/ui/worker-activity.ts';
import {runtimeMetadata} from '../../src/ui/runtime-metadata.ts';

// Presentation for worker events and the live worker widget, composed from
// the companion's presentation library. Pure projection from recorded data;
// the runtime decides what to send, this decides how it reads.

export type {EventDetails} from './record.ts';

export const costText = (dollars: number) =>
  '$' + (dollars < 0.01 ? dollars.toFixed(3) : dollars.toFixed(2));

export interface EventModel {
  heading: Segment[];
  status: Segment[];
  notices: Notice[];
  body: string;
  caption: string;
}

const KIND_TONE: Record<EventKind, Tone> = {
  completed: 'muted',
  interrupted: 'warning',
  unknown: 'warning',
  failed: 'error',
  stopped: 'warning',
  asking: 'warning',
  'context-pressure': 'warning',
  message: 'muted',
  note: 'dim',
  update: 'dim',
  undeliverable: 'error',
  paused: 'dim',
  relay: 'dim',
  running: 'muted',
};
const isKind = (kind: string): kind is EventKind =>
  Object.hasOwn(KIND_TONE, kind);

/** The event body after the runtime's one-line header; headers are rebuilt from details. */
function bodyOf(content: CustomMessage<unknown>['content']): string {
  const text =
    typeof content === 'string'
      ? content
      : content
          .map(block => (block.type === 'text' ? block.text : ''))
          .filter(Boolean)
          .join('\n');
  const split = text.indexOf('\n\n');
  return split === -1 ? text : text.slice(split + 2);
}

export function projectEvent(message: CustomMessage<EventDetails>): EventModel {
  const details = message.details ?? {id: '?', kind: 'event'};
  const kind = labelText(details.kind);
  const heading: Segment[] = [
    ...activityHeader('worker', labelText(details.label ?? details.id), [])
      .start,
  ];
  const status: Segment[] = [
    {
      text: kind,
      tone: isKind(kind) ? KIND_TONE[kind] : 'muted',
      priority: 100,
      minWidth: Math.min(9, kind.length),
      shrink: 'clip',
    },
    {
      text: labelText(details.id).slice(0, 12),
      tone: 'dim',
      priority: 10,
      shrink: 'omit',
    },
  ];
  if (details.assignment)
    status.push({
      text: `task ${labelText(details.assignment.id)}`,
      tone: 'dim',
      priority: 50,
      shrink: 'omit',
    });
  if (details.elapsed)
    status.push({
      text: labelText(details.elapsed),
      tone: 'dim',
      priority: 40,
      shrink: 'omit',
    });
  if (typeof details.tokens === 'number')
    status.push({
      text: `${details.tokens.toLocaleString('en-US')} tok`,
      tone: 'dim',
      priority: 20,
      shrink: 'omit',
    });
  if (typeof details.context === 'number')
    status.push({
      ...contextMeter(details.context),
      text: `${details.contextBasis === 'compact' ? 'compact ' : ''}${contextMeter(details.context).text}`,
      priority: 30,
      shrink: 'omit',
    });
  if (typeof details.cost === 'number')
    status.push({
      text: costText(details.cost),
      tone: 'dim',
      priority: 15,
      shrink: 'omit',
    });
  const notices: Notice[] = [];
  if (kind === 'asking')
    notices.push({
      text: `answer with message { to: "${details.id}", text }`,
      severity: 'warning',
    });
  if (kind === 'undeliverable')
    notices.push({
      text: 'message could not be routed; see body',
      severity: 'error',
    });
  if (details.truncated)
    notices.push({
      text: `excerpt; full text: worker_status { id: "${details.id}" }`,
      severity: 'dim',
    });
  return {
    heading,
    status,
    notices,
    body: bodyOf(message.content),
    caption: details.session ? `session ${details.session}` : '',
  };
}

/** The same identity/outcome header as tools, with a bounded result preview. */
export function renderEvent(
  message: CustomMessage<EventDetails>,
  options: MessageRenderOptions,
  theme: Theme,
): Component {
  const model = projectEvent(message);
  const card = new ActivityCard();
  const kind = message.details?.kind ?? '';
  const state: ActivityState | undefined =
    kind === 'completed'
      ? 'done'
      : kind === 'failed' || kind === 'undeliverable'
        ? 'failed'
        : kind === 'stopped' || kind === 'interrupted'
          ? 'cancelled'
          : kind === 'asking' || kind === 'unknown'
            ? 'waiting'
            : undefined;
  card.update(
    {
      header: activityHeader(
        'worker',
        model.heading[1]?.text ?? '',
        model.status.slice(0, 1),
        'end',
        model.status.slice(1),
      ),
      notices: model.notices.map(notice =>
        !options.expanded && notice.text.startsWith('excerpt;')
          ? {...notice, text: 'Excerpt · worker_status for full result'}
          : notice,
      ),
      body: model.body
        ? new MarkdownView(model.body, theme, options.expanded ? undefined : 3)
        : undefined,
      caption: options.expanded ? model.caption : '',
      ...(state ? {state} : {}),
    },
    theme,
  );
  const pad = Math.max(0, options.outputPad);
  return {
    render: width =>
      card.render(width - 2 * pad).map(line => ' '.repeat(pad) + line),
    invalidate: () => card.invalidate(),
  };
}

/** One line of what a tool call is doing, from its name and arguments. Bounded, sanitized. */
export function describeActivity(toolName: string, args: unknown): string {
  const a = (args && typeof args === 'object' ? args : {}) as Record<
    string,
    unknown
  >;
  const str = (key: string) =>
    typeof a[key] === 'string' ? labelText(a[key] as string) : '';
  const first = () => {
    for (const v of Object.values(a))
      if (typeof v === 'string' && v) return labelText(v);
    return '';
  };
  const detail = (() => {
    switch (toolName) {
      case 'bash':
        return str('command');
      case 'read':
      case 'write':
      case 'edit':
      case 'ls':
        return str('path');
      case 'grep':
      case 'find':
        return [str('pattern'), str('path')].filter(Boolean).join(' in ');
      case 'worker_spawn':
        return str('label');
      case 'message':
        return `to ${str('to')}`;
      case 'worker_status':
      case 'worker_wait':
      case 'worker_stop':
        return str('id');
      case 'codemode':
        return 'script';
      default:
        return first();
    }
  })();
  return detail ? `${toolName} ${detail.slice(0, 200)}` : toolName;
}

/** The first non-empty line of an assistant message's text, as a glance at what it is saying. */
export function describeText(message: unknown): string {
  const m = message as {role?: string; content?: unknown} | undefined;
  if (!m || m.role !== 'assistant' || !Array.isArray(m.content)) return '';
  for (const block of m.content as {type?: string; text?: string}[]) {
    if (block.type === 'text' && block.text) {
      const line = block.text.split('\n').find(l => l.trim());
      if (line) return labelText(line).slice(0, 200);
    }
  }
  return '';
}

export interface WidgetRow {
  label: string;
  model?: string | undefined;
  thinking?: string | undefined;
  /** Observation metadata; never rendered as a claim of execution. */
  assignment?: TaskAssignment;
  state: string;
  /** 0 for workers this session owns; descendants read from the registry nest beneath. */
  depth: number;
  elapsed: string;
  tokens: number;
  calls: number;
  /** Current tool call, or what it last said/did when between calls. */
  detail: string;
  /** What it is working toward: its latest reasoning heading or progress note. */
  headline?: string | undefined;
  /** Verified checkout identity, omitted when shared with the inspecting session. */
  checkout?: {kind: 'branch' | 'directory'; label: string} | undefined;
  /** Compacting or retrying; outranks the headline while it lasts. */
  phase?: string | undefined;
  /** Share of the displayed budget, with capacity as the default basis. */
  context?: number | undefined;
  contextBasis?: 'compact' | undefined;
  /** Recently finished: shown dimmed with its outcome, then dropped. */
  finished?: boolean | undefined;
  /** Set when the last tool call errored; cleared by the next successful one. */
  failed?: string | undefined;
  /** Milliseconds since the child produced any event. */
  idleMs: number;
}

/** A worker that has produced nothing for this long is shown as quiet (a stall hint, not a verdict). */
export const STALL_MS = 90_000;

function formatMs(ms: number): string {
  const s = Math.round(ms / 1000);
  return s < 60
    ? `${s}s`
    : s < 3600
      ? `${Math.floor(s / 60)}m`
      : `${Math.floor(s / 3600)}h${Math.floor((s % 3600) / 60)}m`;
}

/** 950 → "950", 10_666 → "10.7k", 1_250_000 → "1.3M". */
export function compact(n: number): string {
  if (n < 1000) return String(n);
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

const quiet = (row: WidgetRow) =>
  row.state === 'running' && row.idleMs >= STALL_MS;

/** Attention first: asking, then a failed last call, then quiet, then oldest. Stable within a tree level. */
export function attentionOrder(a: WidgetRow, b: WidgetRow): number {
  const rank = (r: WidgetRow) =>
    r.finished ? 4 : r.state === 'asking' ? 0 : r.failed ? 1 : quiet(r) ? 2 : 3;
  return rank(a) - rank(b);
}

export const DEFAULT_WIDGET_LINES = activityRows.workers;
export {WORKER_FRAME_MS, workerFrame} from '../../src/ui/worker-activity.ts';

/** Live tree: identity/intent above activity; terminal workers take one line.
 * The owner drives animation. Rendering never starts timers or observes processes. */
export function createWidget(
  rows: () => WidgetRow[],
  theme: Theme,
  maxLines: number = DEFAULT_WIDGET_LINES,
  frame: () => number = () => 0,
): Component {
  const stack = new VStack();
  const section = new ActivitySection();
  return {
    render(width) {
      const all = rows();
      if (all.length === 0 || width <= 0) return [];
      stack.clear();
      const height = (row: WidgetRow) =>
        row.state === 'running' || row.state === 'asking' ? 2 : 1;
      const budget = Math.max(3, maxLines);
      const overflow = all.reduce((sum, row) => sum + height(row), 0) > budget;
      // Reserve the summary before picking whole workers; never show half a row.
      let remaining = budget - (overflow ? 1 : 0);
      const shown: WidgetRow[] = [];
      for (const row of all) {
        if (height(row) > remaining) break;
        shown.push(row);
        remaining -= height(row);
      }
      // Keeping the prefix preserves every displayed descendant's ancestry.
      const column = Math.min(
        24,
        Math.max(
          ...shown.map(r => r.depth * 2 + visibleWidth(labelText(r.label))),
        ),
      );
      // Auxiliary metadata yields before the shared runtime lane.
      const metadataBudget = Math.floor(Math.max(0, width - 2) / 2);
      const elapsedWidth = Math.max(
        ...shown.map(row => visibleWidth(labelText(row.elapsed))),
      );
      const checkoutText = (row: WidgetRow) =>
        row.checkout
          ? `${row.checkout.kind === 'branch' ? '⎇' : '📁'} ${labelText(row.checkout.label)}`
          : '';
      const checkoutWidth = Math.max(
        ...shown.map(row => visibleWidth(checkoutText(row))),
      );
      for (const row of shown) {
        const asking = row.state === 'asking';
        const stalled = quiet(row);
        const live = row.state === 'running' || asking;
        const outcome =
          row.state === 'completed'
            ? 'done'
            : row.state === 'failed'
              ? 'failed'
              : row.state === 'unknown'
                ? 'waiting'
                : 'cancelled';
        const decoration = activityDecoration(
          row.finished
            ? outcome
            : asking
              ? 'waiting'
              : row.failed
                ? 'failed'
                : 'running',
        );
        const glyph =
          row.state === 'completed'
            ? '✓'
            : row.state === 'running' && !row.failed && !row.finished
              ? workerFrame(frame())
              : live || row.finished
                ? decoration.glyph
                : '○';
        const glyphTone: Tone = row.finished
          ? decoration.tone
          : !live
            ? 'dim'
            : stalled
              ? 'warning'
              : decoration.tone;
        const indent = row.depth ? '\u2937 ' : '';
        const label = labelText(row.label);
        const padTo = Math.max(visibleWidth(label), column - row.depth * 2);
        const end: Segment[] = [];
        if (stalled)
          end.push({
            text: `quiet ${formatMs(row.idleMs)}`,
            tone: 'warning',
            priority: 65,
            shrink: 'omit',
          });
        if (row.checkout)
          end.push({
            text: checkoutText(row),
            width: checkoutWidth,
            tone: 'muted',
            priority: 40,
            shrink: 'omit',
          });
        end.push({
          text: row.elapsed,
          width: elapsedWidth,
          tone: 'dim',
          priority: 60,
          shrink: 'omit',
        });
        const endWidth = () =>
          end.reduce(
            (sum, segment) =>
              sum + (segment.width ?? visibleWidth(labelText(segment.text))),
            0,
          ) +
          Math.max(0, end.length - 1) * 3;
        while (end.length && endWidth() > metadataBudget) {
          const lowest = end.reduce(
            (best, segment, index) =>
              segment.priority < end[best]!.priority ? index : best,
            0,
          );
          end.splice(lowest, 1);
        }
        end.push(
          ...runtimeMetadata({
            model: labelText(
              row.model?.split('/').slice(1).join('/') ||
                row.model ||
                'model ?',
            ),
            thinking: row.thinking,
            contextPercent: row.context,
          }),
        );
        const line = new PriorityRow();
        line.update(
          {
            start: [
              {
                text: `${indent}${glyph}`,
                tone: glyphTone,
                priority: 100,
                shrink: 'clip',
              },
              {
                text: label,
                tone: 'toolTitle',
                bold: true,
                priority: 110,
                shrink: 'clip',
                clip: 'end',
                minWidth: 8,
                padTo,
              },
              ...(row.failed
                ? [
                    {
                      text: `${row.failed} failed`,
                      tone: 'error' as const,
                      priority: 95,
                      shrink: 'clip' as const,
                      clip: 'end' as const,
                      minWidth: 6,
                    },
                  ]
                : []),
              ...(row.headline
                ? [
                    {
                      text: row.headline,
                      tone: 'text' as const,
                      priority: 52,
                      shrink: 'clip' as const,
                      clip: 'end' as const,
                      minWidth: 10,
                    },
                  ]
                : []),
              ...(!live
                ? [
                    {
                      text: row.state,
                      tone: (row.state === 'failed' ? 'error' : 'dim') as Tone,
                      priority: 54,
                      shrink: 'clip' as const,
                      clip: 'end' as const,
                      minWidth: 6,
                    },
                    {
                      text: `· ${row.detail}`,
                      tone: 'dim' as const,
                      priority: 50,
                      shrink: 'clip' as const,
                      clip: 'end' as const,
                      minWidth: 12,
                    },
                  ]
                : []),
            ],
            end,
            indent: row.depth > 1 ? (row.depth - 1) * 2 : 0,
            separator: ' ',
            endSeparator: '  ',
          },
          theme,
        );
        stack.addChild(line);
        if (live) {
          const activity = new PriorityRow();
          activity.update(
            {
              start: [
                {text: '└', tone: 'dim', priority: 100, shrink: 'clip'},
                {
                  text: row.phase ?? row.detail,
                  tone: asking || row.phase ? 'warning' : 'dim',
                  priority: 90,
                  shrink: 'clip',
                  clip: 'end',
                  minWidth: 1,
                },
              ],
              end: [],
              indent: row.depth * 2 + 2,
              separator: ' ',
            },
            theme,
          );
          stack.addChild(activity);
        }
      }
      if (all.length > shown.length) {
        const more = new PriorityRow();
        more.update(
          {
            start: [
              {
                text: `+${all.length - shown.length} more \u00b7 /workers`,
                tone: 'dim',
                priority: 100,
                shrink: 'clip',
              },
            ],
            end: [],
          },
          theme,
        );
        stack.addChild(more);
      }
      section.update(
        {heading: activityHeading('Workers', '/workers'), body: stack},
        theme,
      );
      return section.render(width);
    },
    invalidate() {
      section.invalidate();
    },
  };
}
