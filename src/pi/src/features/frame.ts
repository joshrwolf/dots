import type {Theme} from '@earendil-works/pi-coding-agent';
import type {Component} from '@earendil-works/pi-tui';
import {PriorityRow, type Segment} from '@joshrwolf/pi-kit/ui';
import {
  statusItems,
  renderStatusRow,
  type StatusItem,
} from '../ui/status-row.ts';
import {runtimeMetadata} from '../ui/runtime-metadata.ts';

export interface FooterModel {
  readonly project: string;
  readonly branch?: string | undefined;
  readonly model: string;
  readonly thinking?: string | undefined;
  readonly contextPercent?: number | undefined;
  readonly statuses: readonly StatusItem[];
}

/** One source for model/context information; no clocks, probes or session reads. */
export class FooterView implements Component {
  private readonly row = new PriorityRow();
  private statuses: readonly StatusItem[] = [];
  update(model: FooterModel, theme: Theme): void {
    const start: Segment[] = [
      {
        text: model.project,
        tone: 'accent',
        priority: 40,
        shrink: 'clip',
        clip: 'middle',
        minWidth: 10,
      },
    ];
    if (model.branch)
      start.push({
        text: `⑂ ${model.branch}`,
        tone: 'muted',
        priority: 20,
        shrink: 'omit',
      });
    const end = runtimeMetadata({...model, model: model.model || 'no model'});
    this.row.update({layout: 'edges', start, end, separator: '  '}, theme);
    this.statuses = statusItems(model.statuses);
  }
  invalidate(): void {
    this.row.invalidate();
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    const lines = this.row.render(width);
    lines.push(...renderStatusRow(this.statuses, width));
    return lines;
  }
}
