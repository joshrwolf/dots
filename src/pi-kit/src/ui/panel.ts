import type {Theme} from '@earendil-works/pi-coding-agent';
import {
  sliceByColumn,
  stripTerminalSequences,
  truncateToWidth,
  visibleWidth,
  type Component,
  type TuiMouseEvent,
  type TuiMouseEventResult,
} from '@earendil-works/pi-tui';
import {PriorityRow, type RowModel} from './row.ts';
import {paintSurfaceLine} from './surface.ts';

/**
 * Pi's message components start lines with OSC 133 prompt marks, which the
 * host strips only from the start of the lines it draws. Framed, they would
 * reach the terminal mid-line, where it reads 133;A as a new shell prompt and
 * breaks the row.
 */
const promptMarks = /^(?:\x1b\]133;[ABC](?:\x07|\x1b\\))+/;
const renderBody = (body: Component, width: number): string[] =>
  body.render(width).map(line => line.replace(promptMarks, ''));

export interface PanelModel {
  readonly heading: RowModel;
  readonly body: Component;
  readonly footer?: RowModel;
}

/** Presentation-only frame for a dashboard or interactive inspection body.
 * The body retains input, selection, and scrolling; the frame forwards mouse
 * coordinates to it without owning those interactions. */
export class Panel implements Component {
  private readonly heading = new PriorityRow();
  private readonly footer = new PriorityRow();
  private model: PanelModel | undefined;
  private theme: Theme | undefined;
  private bodyHeight = 0;
  private bodyWidth = 0;
  private bodyTop = 0;

  update(model: PanelModel, theme: Theme): void {
    this.model = model;
    this.theme = theme;
    this.heading.update(model.heading, theme);
    this.footer.update(model.footer ?? {start: [], end: []}, theme);
    this.invalidate();
  }
  invalidate(): void {
    this.heading.invalidate();
    this.footer.invalidate();
    this.model?.body.invalidate();
  }
  render(width: number): string[] {
    const model = this.model;
    const theme = this.theme;
    if (!model || !theme || width <= 0) return [];
    const colors = {
      fg: theme.getFgAnsi('text'),
      bg: theme.getBgAnsi('customMessageBg'),
    };
    // Child styles and links end before frame-owned padding and borders.
    const boundary = '\x1b[0m\x1b]8;;\x07';
    // At very narrow widths, spend the cells on content rather than decoration.
    if (width < 12) {
      this.bodyTop = this.heading.render(width).length;
      this.bodyWidth = width;
      const body = renderBody(model.body, width);
      this.bodyHeight = body.length;
      return [
        ...this.heading.render(width),
        ...body,
        ...this.footer.render(width),
      ].map(line => paintSurfaceLine(line, width, colors));
    }
    const inner = width - 4;
    this.bodyTop = 1;
    this.bodyWidth = inner;
    const border = (text: string) => theme.fg('borderMuted', text);
    const edge = (row: PriorityRow, left: string, right: string): string => {
      const line = row.render(inner - 1)[0] ?? '';
      const cells = visibleWidth(stripTerminalSequences(line).trimEnd());
      const label = sliceByColumn(line, 0, cells, true);
      return (
        border(`${left}─`) +
        (cells ? ` ${label}${boundary} ` : '──') +
        border('─'.repeat(Math.max(0, inner - 1 - cells)) + right)
      );
    };
    const bodyLines = renderBody(model.body, inner);
    this.bodyHeight = bodyLines.length;
    const body = bodyLines.map(line => {
      const bounded = truncateToWidth(line, inner, '');
      return (
        border('│ ') +
        bounded +
        boundary +
        ' '.repeat(Math.max(0, inner - visibleWidth(bounded))) +
        border(' │')
      );
    });
    return [
      edge(this.heading, '╭', '╮'),
      ...body,
      edge(this.footer, '╰', '╯'),
    ].map(line => paintSurfaceLine(line, width, colors));
  }

  handleMouse(event: TuiMouseEvent): TuiMouseEventResult | undefined {
    const body = this.model?.body;
    if (
      !body ||
      event.y < this.bodyTop ||
      event.y >= this.bodyTop + this.bodyHeight
    )
      return undefined;
    const inset = event.width < 12 ? 0 : 2;
    if (event.x < inset || event.x >= inset + this.bodyWidth) return undefined;
    return body.handleMouse?.({
      ...event,
      x: event.x - inset,
      y: event.y - this.bodyTop,
      width: this.bodyWidth,
      height: this.bodyHeight,
    });
  }
}
