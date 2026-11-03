// What every event trigger shares: outcomes, transitions, and the quoting of
// text other programs wrote. Each trigger's rules module (pane.ts, task.ts)
// extends a transition with what its messages report.

export type Outcome = 'matched' | 'ended' | 'timed out';

export interface Transition {
  readonly outcome: Outcome;
  /** One line in our own words; text other programs wrote never appears here. */
  readonly summary: string;
  /** The source can produce nothing more: the wake finishes with this fire. */
  readonly final: boolean;
}

const INVISIBLE = /[\p{Cc}\u200b-\u200f\u202a-\u202e\u2060-\u2069\ufeff]/gu;

/** Text another program wrote, as a bounded, quoted literal with controls and direction overrides removed. */
export function untrusted(text: string, max = 300): string {
  const clean = text.replace(INVISIBLE, '');
  return JSON.stringify(clean.length > max ? `${clean.slice(0, max)}…` : clean);
}

export const untilLabel = (until: readonly string[]) => until.join('|');
