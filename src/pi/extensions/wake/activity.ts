import type {Theme} from '@earendil-works/pi-coding-agent';
import {Text, VStack, type Component} from '@earendil-works/pi-tui';
import {PriorityRow, labelText, type Segment} from '../../src/index.ts';
import {
  ActivitySection,
  activityHeading,
  activityRows,
} from '../../src/ui/activity-section.ts';

interface ActivityBase {
  readonly id: string;
  readonly text: string;
  readonly fired: number;
  readonly limit: number;
}

export interface TimeActivity extends ActivityBase {
  readonly kind: 'time';
  readonly at: number;
  readonly every?: number | undefined;
  readonly check?: string | undefined;
  readonly on?: string | undefined;
  readonly lastCode?: number | undefined;
  readonly state: 'scheduled' | 'checking' | 'queued';
}

export interface TaskActivity extends ActivityBase {
  readonly kind: 'task';
  readonly task: string;
  /** The root's title when the wake was set; writer-controlled text. */
  readonly title: string;
  readonly project: string;
  readonly until: readonly string[];
  readonly deadline: number;
  /** The last task store event seen. */
  readonly cursor: number;
  /** Why the task store cannot be observed right now. */
  readonly disconnected?: string;
  readonly state: 'waiting' | 'queued';
}

/** Observations only: presentation never arms or fires a wake. */
export type WakeActivity = TimeActivity | TaskActivity;

export function wakeDelay(ms: number): string {
  const seconds = Math.ceil(Math.max(0, ms) / 1000);
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.ceil(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.ceil(seconds / 3600)}h`;
  return `${Math.ceil(seconds / 86400)}d`;
}

const due = (row: WakeActivity) =>
  row.kind === 'time' ? row.at : row.deadline;

export function orderWakes(rows: readonly WakeActivity[]): WakeActivity[] {
  const rank = (row: WakeActivity) =>
    row.state === 'checking' ? 0 : row.state === 'queued' ? 1 : 2;
  return [...rows].sort(
    (a, b) => rank(a) - rank(b) || due(a) - due(b) || a.id.localeCompare(b.id),
  );
}

function displayText(text: string, limit: number): string {
  return (
    labelText(text.slice(0, limit)) +
    (text.length > limit ? ' … [display clipped]' : '')
  );
}

function status(row: WakeActivity, now: number): string {
  if (row.kind !== 'time')
    return row.state === 'queued'
      ? 'queued'
      : `waiting for task ${labelText(row.task)}: until ${row.until.join('|')}`;
  return row.state === 'scheduled'
    ? row.at <= now
      ? 'due'
      : `in ${wakeDelay(row.at - now)}`
    : row.state;
}

function taskEnd(row: TaskActivity, now: number): Segment[] {
  const end: Segment[] = [];
  if (row.disconnected !== undefined)
    end.push({
      text: 'disconnected',
      tone: 'warning',
      priority: 95,
      shrink: 'omit',
    });
  if (row.state === 'waiting')
    end.push({
      text: `${wakeDelay(row.deadline - now)} left`,
      tone: 'dim',
      priority: 40,
      shrink: 'omit',
    });
  if (row.limit > 1)
    end.push({
      text: `${row.fired}/${row.limit}`,
      tone: 'dim',
      priority: 30,
      shrink: 'omit',
    });
  return end;
}

function timeEnd(row: TimeActivity): Segment[] {
  const end: Segment[] = [];
  if (row.every)
    end.push({
      text: `↻ ${wakeDelay(row.every)} · ${row.fired}/${row.limit}`,
      tone: 'dim',
      priority: 30,
      shrink: 'omit',
    });
  if (row.lastCode !== undefined && row.lastCode !== 0)
    end.push({
      text: `exit ${row.lastCode}`,
      tone: 'error',
      priority: 95,
      shrink: 'omit',
    });
  return end;
}

function details(row: WakeActivity): string[] {
  if (row.kind === 'task')
    return [
      `Task: ${labelText(row.task)} ${displayText(JSON.stringify(row.title), 240)} · ${labelText(row.project)}`,
      `Until: ${row.until.join('|')} on the task or a descendant · gives up ${new Date(row.deadline).toISOString()}`,
      `Seen through event #${row.cursor}`,
      ...(row.disconnected === undefined
        ? []
        : [`Disconnected: ${displayText(row.disconnected, 4000)}`]),
    ];
  return [
    `Scheduled: ${new Date(row.at).toISOString()}${row.every ? ` · every ${wakeDelay(row.every)}` : ' · once'}`,
    ...(row.check
      ? [
          `Check (${row.on ?? 'change'}): ${displayText(row.check, 4000)}`,
          `Last exit: ${row.lastCode ?? 'not observed'}`,
        ]
      : []),
  ];
}

/** One shared projection for the persistent section and full inspection. */
export function wakeActivityBody(
  rows: readonly WakeActivity[],
  now: number,
  theme: Theme,
  detailed = false,
): Component {
  const body = new VStack();
  if (!rows.length)
    body.addChild(new Text(theme.fg('dim', 'No active wakes.'), 0, 0));
  for (const row of rows) {
    const end: Segment[] = [
      {
        text: status(row, now),
        tone: row.state === 'checking' ? 'accent' : 'muted',
        priority: 100,
        shrink: 'clip',
        minWidth: 4,
      },
      ...(row.kind === 'time' ? timeEnd(row) : taskEnd(row, now)),
    ];
    const line = new PriorityRow();
    line.update(
      {
        start: [
          {
            text: displayText(row.text, 240),
            tone: 'text',
            priority: 90,
            shrink: 'clip',
            clip: 'end',
            minWidth: 8,
          },
        ],
        end,
        separator: ' ',
        endSeparator: ' · ',
      },
      theme,
    );
    body.addChild(line);
    if (detailed) {
      const fields = [
        displayText(row.text, 4000),
        `${row.id} · ${row.state} · ${row.fired}/${row.limit} fires`,
        ...details(row),
        `Cancel: wake_cancel { id: "${row.id}" }`,
      ];
      body.addChild(
        new Text(
          fields.map(field => theme.fg('muted', field)).join('\n') + '\n',
          0,
          0,
        ),
      );
    }
  }
  return body;
}

export function createWakeWidget(
  rows: () => readonly WakeActivity[],
  now: () => number,
  theme: Theme,
): Component {
  const section = new ActivitySection();
  return {
    render(width) {
      const all = orderWakes(rows());
      if (!all.length || width <= 0) return [];
      section.update(
        {
          heading: activityHeading('Wakes', '/wakes'),
          body: wakeActivityBody(
            all.slice(0, activityRows.wakes),
            now(),
            theme,
          ),
          ...(all.length > activityRows.wakes
            ? {
                footer: {
                  start: [
                    {
                      text: `+${all.length - activityRows.wakes} more · /wakes`,
                      tone: 'dim',
                      priority: 100,
                      shrink: 'clip',
                    },
                  ],
                  end: [],
                },
              }
            : {}),
        },
        theme,
      );
      return section.render(width);
    },
    invalidate: () => section.invalidate(),
  };
}
