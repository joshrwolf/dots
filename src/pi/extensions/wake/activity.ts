import type {Theme} from '@earendil-works/pi-coding-agent';
import {Text, VStack, type Component} from '@earendil-works/pi-tui';
import {PriorityRow, labelText, type Segment} from '../../src/index.ts';
import {
  ActivitySection,
  activityHeading,
  activityRows,
} from '../../src/ui/activity-section.ts';

/** Observations only: presentation never arms or fires a wake. */
export interface WakeActivity {
  readonly id: string;
  readonly text: string;
  readonly at: number;
  readonly every?: number | undefined;
  readonly fired: number;
  readonly limit: number;
  readonly check?: string | undefined;
  readonly on: string;
  readonly lastCode?: number | undefined;
  readonly state: 'scheduled' | 'checking' | 'queued';
}

export function wakeDelay(ms: number): string {
  const seconds = Math.ceil(Math.max(0, ms) / 1000);
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.ceil(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.ceil(seconds / 3600)}h`;
  return `${Math.ceil(seconds / 86400)}d`;
}

export function orderWakes(rows: readonly WakeActivity[]): WakeActivity[] {
  const rank = (row: WakeActivity) =>
    row.state === 'checking' ? 0 : row.state === 'queued' ? 1 : 2;
  return [...rows].sort(
    (a, b) => rank(a) - rank(b) || a.at - b.at || a.id.localeCompare(b.id),
  );
}

function displayText(text: string, limit: number): string {
  return (
    labelText(text.slice(0, limit)) +
    (text.length > limit ? ' … [display clipped]' : '')
  );
}

function status(row: WakeActivity, now: number): string {
  return row.state === 'scheduled'
    ? row.at <= now
      ? 'due'
      : `in ${wakeDelay(row.at - now)}`
    : row.state;
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
    ];
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
        `Scheduled: ${new Date(row.at).toISOString()}${row.every ? ` · every ${wakeDelay(row.every)}` : ' · once'}`,
        ...(row.check
          ? [
              `Check (${row.on}): ${displayText(row.check, 4000)}`,
              `Last exit: ${row.lastCode ?? 'not observed'}`,
            ]
          : []),
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
