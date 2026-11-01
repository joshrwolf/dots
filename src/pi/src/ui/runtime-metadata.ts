import type {Segment} from '@joshrwolf/pi-kit/ui';
import {contextMeter} from './context.ts';

export interface RuntimeMetadata {
  readonly model: string;
  readonly thinking?: string | undefined;
  readonly contextPercent?: number | undefined;
}

/** The primary session and worker rows share a right-anchored model/effort/context lane.
 * Fixed trailing cells align different effort levels and percentages; native row
 * priorities release these columns when the terminal cannot fit them. */
export function runtimeMetadata(model: RuntimeMetadata): Segment[] {
  const meter = contextMeter(model.contextPercent);
  return [
    {
      text: model.model,
      tone: 'text',
      priority: 70,
      shrink: 'clip',
      clip: 'middle',
      minWidth: 8,
    },
    {
      text: `✦ ${model.thinking ?? '?'}`,
      padTo: 9,
      tone: 'muted',
      priority: 10,
      shrink: 'omit',
    },
    {text: meter.bar, tone: meter.tone, priority: 30, shrink: 'omit'},
    {
      text: meter.value,
      padTo: 4,
      minWidth: meter.value.length,
      tone: meter.tone,
      priority: 100,
      shrink: 'clip',
      separatorBefore: ' ',
    },
  ];
}
