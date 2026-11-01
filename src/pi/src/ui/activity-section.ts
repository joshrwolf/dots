import type {Theme} from '@earendil-works/pi-coding-agent';
import {VStack, type Component} from '@earendil-works/pi-tui';
import {
  PriorityRow,
  type RowModel,
  type PanelModel,
} from '@joshrwolf/pi-kit/ui';

/** Persistent sections share a small default footprint; inspectors keep full detail.
 * Feature owners select relevant records and report omissions, never clip away evidence here. */
export {activityRows} from './policy.ts';

export function activityHeading(
  kind: 'Goal' | 'Goals' | 'Workers' | 'Jobs' | 'Wakes',
  command: string,
  title = '',
): RowModel {
  const icon =
    kind === 'Goal' || kind === 'Goals'
      ? '◎'
      : kind === 'Workers'
        ? '◇'
        : kind === 'Wakes'
          ? '◷'
          : '▷';
  return {
    divider: true,
    start: [
      {
        text: `${icon} ${kind}`,
        tone: 'accent',
        bold: true,
        priority: 100,
        shrink: 'clip',
      },
      {text: title, tone: 'text', priority: 80, shrink: 'clip', minWidth: 8},
    ],
    end: [{text: command, tone: 'muted', priority: 20, shrink: 'omit'}],
    separator: '  ',
  };
}

/** Unboxed persistent chrome. Native layout and feature-owned rows do the work. */
export class ActivitySection extends VStack {
  update(model: PanelModel, theme: Theme): void {
    this.clear();
    const heading = new PriorityRow();
    heading.update(model.heading, theme);
    this.addChild(heading);
    const content = new VStack();
    content.addChild(model.body);
    if (model.footer) {
      const footer = new PriorityRow();
      footer.update(model.footer, theme);
      content.addChild(footer);
    }
    const indented: Component = {
      render(width) {
        if (width <= 0) return [];
        const indent = Math.min(2, width - 1);
        return content
          .render(width - indent)
          .map(line => ' '.repeat(indent) + line);
      },
      invalidate() {
        content.invalidate();
      },
    };
    this.addChild(indented);
    this.invalidate();
  }
  override render(width: number): string[] {
    return width > 0 ? super.render(width) : [];
  }
}
