import type {Theme} from '@earendil-works/pi-coding-agent';
import {truncateToWidth, type Component} from '@earendil-works/pi-tui';

import {workerFrame, WORKER_FRAME_MS} from './worker-activity.ts';

export type ActivityState =
  | 'receiving'
  | 'queued'
  | 'running'
  | 'done'
  | 'failed'
  | 'cancelled'
  | 'unknown'
  | 'waiting';
const glyphs: Record<ActivityState, string> = {
  receiving: '·',
  queued: '◦',
  running: '●',
  done: ' ',
  failed: '✗',
  cancelled: '−',
  unknown: '?',
  waiting: '?',
};

export function activityDecoration(state: ActivityState) {
  const tone =
    state === 'failed'
      ? 'error'
      : state === 'running'
        ? 'accent'
        : state === 'waiting' || state === 'unknown'
          ? 'warning'
          : 'muted';
  return {glyph: glyphs[state], tone} as const;
}

/** Running cues advance on owner redraws; rows never start their own timers. */
export class StateGutter implements Component {
  private readonly child: Component;
  private readonly state: ActivityState;
  private readonly theme: Theme;
  private readonly expanded: boolean | undefined;
  constructor(
    child: Component,
    state: ActivityState,
    theme: Theme,
    expanded?: boolean,
  ) {
    this.child = child;
    this.state = state;
    this.theme = theme;
    this.expanded = expanded;
  }
  invalidate(): void {
    this.child.invalidate();
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    const {glyph, tone} = activityDecoration(this.state);
    const mark =
      this.state === 'done' && this.expanded !== undefined
        ? this.expanded
          ? '▾'
          : '▸'
        : this.state === 'running'
          ? workerFrame(Math.floor(Date.now() / WORKER_FRAME_MS))
          : glyph;
    const prefix = this.theme.fg(tone, `${mark} `);
    if (width <= 2) return [truncateToWidth(prefix, width)];
    return this.child
      .render(width - 2)
      .map((line, index) => (index === 0 ? prefix : '  ') + line);
  }
}
