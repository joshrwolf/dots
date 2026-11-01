import type {Segment, Tone} from '@joshrwolf/pi-kit/ui';

/** From here context use draws attention: Pi compacts as the window fills. */
export const CONTEXT_WARN = 75;
/** From here context use is urgent. */
export const CONTEXT_FULL = 90;

const CELLS = 10;

/** Anthropic 1M meters use the effective native trigger; other models keep capacity meters. */
export function compactionMeterLimit(
  model: string | undefined,
  window: number | undefined,
  compactAt: number | null | undefined,
): number | undefined {
  return model?.startsWith('anthropic/') &&
    window !== undefined &&
    window >= 1_000_000 &&
    typeof compactAt === 'number' &&
    compactAt > 0
    ? compactAt
    : undefined;
}

/**
 * Context use at a glance, the same everywhere it is shown: a thin line that
 * fills in tenths, heavy over a light track, then the percent, in foreground
 * color only. The percent is
 * the fact; the bar is the first thing to go when space runs out. Unknown use
 * (right after compaction) shows an empty bar and `?`.
 */
export function contextMeter(percent: number | undefined): {
  text: string;
  bar: string;
  value: string;
  tone: Tone;
} {
  const known = percent !== undefined && Number.isFinite(percent);
  const share = known ? Math.max(0, Math.min(100, percent)) : 0;
  const filled = known ? Math.min(CELLS, Math.round((share / 100) * CELLS)) : 0;
  const bar = `${'━'.repeat(filled)}${'─'.repeat(CELLS - filled)}`;
  const value = known ? `${Math.round(share)}%` : '?';
  const tone: Tone =
    share >= CONTEXT_FULL
      ? 'error'
      : share >= CONTEXT_WARN
        ? 'warning'
        : 'muted';
  return {text: `${bar} ${value}`, bar, value, tone};
}

/**
 * The meter as row segments: the percent keeps its place at `priority`; the
 * bar is decoration at `barPriority`, so a row can let it go before its own
 * content.
 */
export function contextSegments(
  percent: number | undefined,
  priority: number,
  barPriority = priority - 10,
  label = '',
): Segment[] {
  const meter = contextMeter(percent);
  return [
    {text: meter.bar, tone: meter.tone, priority: barPriority, shrink: 'omit'},
    {
      text: label ? `${label} ${meter.value}` : meter.value,
      minWidth: label
        ? label.length + 1 + meter.value.length
        : meter.value.length,
      tone: meter.tone,
      priority,
      shrink: 'clip',
      separatorBefore: ' ',
    },
  ];
}
