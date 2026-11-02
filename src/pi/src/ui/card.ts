import {
  displayLimits,
  PriorityRow,
  type RowModel,
  type Segment,
  labelText,
  layoutText,
  plainLines,
  sourceText,
} from '@joshrwolf/pi-kit/ui';
import type {KeybindingHints, Theme} from '@earendil-works/pi-coding-agent';
import {
  VStack,
  stripTerminalSequences,
  visibleWidth,
  type Component,
} from '@earendil-works/pi-tui';
import {StateGutter, type ActivityState} from './state.ts';
import {DetailView} from './details.ts';

/**
 * A disclosure cue in the host's configured expand keys, such as `ctrl+o to
 * expand`. Use the formatters Pi injects into renderers; an imported copy may
 * not see the user's keybindings. Empty when the action has no key.
 */
export function expandCue(
  hints: Pick<KeybindingHints, 'keyText'>,
  action = 'to expand',
): string {
  const key = hints.keyText('app.tools.expand');
  return key ? `${key} ${action}` : '';
}

export interface Notice {
  readonly text: string;
  readonly severity: 'warning' | 'error' | 'dim';
}

export class Notices implements Component {
  private theme: Theme | undefined;
  private values: readonly Notice[] = [];
  update(values: readonly Notice[], theme: Theme): void {
    this.values = values;
    this.theme = theme;
  }
  invalidate(): void {}
  render(width: number): string[] {
    const theme = this.theme;
    if (!theme || width <= 0) return [];
    const inset = width >= 20 ? '  ' : '';
    const room = width - inset.length;
    return this.values.flatMap(notice => {
      const mark =
        room >= 8
          ? notice.severity === 'error'
            ? '↳ '
            : notice.severity === 'warning'
              ? '! '
              : '· '
          : '';
      return plainLines(notice.text, room - mark.length).map(
        (line, index) =>
          inset +
          theme.fg(
            notice.severity,
            (index === 0 ? mark : ' '.repeat(mark.length)) + line,
          ),
      );
    });
  }
}

/** Bounded recorded output, with tail diagnostics retained when the head is cut. */
export class OutputView implements Component {
  private readonly text: string;
  private cache: {width: number; lines: string[]} | undefined;
  constructor(text: string) {
    this.text = sourceText(
      text.length <= displayLimits.sourceCodeUnits
        ? text
        : text.slice(0, displayLimits.sourceCodeUnits / 2) +
            '\n… output display limited …\n' +
            text.slice(-displayLimits.sourceCodeUnits / 2),
    );
  }
  invalidate(): void {
    this.cache = undefined;
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    if (this.cache?.width === width) return this.cache.lines;
    const prepared = layoutText(this.text, width, displayLimits.outputRows);
    let lines = prepared.lines;
    if (prepared.limited) {
      // Work is bounded before wrapping. The suffix is intentionally small so
      // even one-column output retains the final diagnostic.
      const tail = plainLines(
        this.text.slice(
          -Math.max(1, Math.min(displayLimits.outputRows, width * 20)),
        ),
        width,
        displayLimits.outputRows,
      ).slice(-Math.floor(displayLimits.outputRows / 4));
      const marker = lines.at(-1)!;
      lines = [
        ...lines.slice(0, displayLimits.outputRows - tail.length - 1),
        marker,
        ...tail,
      ];
    }
    this.cache = {width, lines};
    return lines;
  }
}

/** A bounded glance at recorded text. Expansion always uses the full OutputView. */
export class PreviewView implements Component {
  private readonly output: OutputView;
  private readonly theme: Theme;
  private readonly rows: number;
  private readonly cue: string;
  constructor(
    text: string,
    theme: Theme,
    hints: Pick<KeybindingHints, 'keyText'>,
    rows = 3,
  ) {
    this.output = new OutputView(text);
    this.theme = theme;
    this.rows = rows;
    this.cue = expandCue(hints);
  }
  invalidate(): void {
    this.output.invalidate();
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    const lines = this.output.render(width);
    return lines.length <= this.rows
      ? lines
      : [
          ...lines.slice(0, this.rows),
          ...plainLines(`… ${this.cue}`.trimEnd(), width, 1).map(line =>
            this.theme.fg('dim', line),
          ),
        ];
  }
}

export interface ActivityCardModel {
  /** Optional static decoration; the caller supplies the semantic state. */
  readonly state?: ActivityState;
  /** Native disclosure state; omitted on non-expandable surfaces. */
  readonly expanded?: boolean;
  readonly notices: readonly Notice[];
  readonly body: Component | undefined;
  readonly caption: string;
  /** Default always; omit a redundant caption only when its full text appears in the rendered header. */
  readonly captionPolicy?: 'always' | 'when-not-in-header';
  readonly header: RowModel;
}

/** Outcomes stay beside the subject; only explicitly secondary metadata spans
 * to the trailing edge. Shrink priorities still protect outcomes in narrow panes. */
export function activityHeader(
  kind: string,
  subject: string,
  status: readonly Segment[],
  clip: NonNullable<Segment['clip']> = 'middle',
  secondary: readonly Segment[] = [],
): RowModel {
  // Bound preparation independently of terminal width; PriorityRow owns clipping.
  const maxUnits = 8192;
  const end =
    subject.length > maxUnits && /[\uD800-\uDBFF]/.test(subject[maxUnits - 1]!)
      ? maxUnits - 1
      : maxUnits;
  const heading =
    labelText(subject.slice(0, end)) +
    (subject.length > maxUnits ? '… [display limited]' : '');
  return {
    layout: 'edges',
    separator: '  ',
    groupSeparator: '  ',
    endSeparator: ' · ',
    start: [
      {
        text: kind,
        tone: 'accent',
        bold: true,
        priority: 90,
        shrink: 'clip',
        padTo: 8,
        minWidth: Math.min(5, visibleWidth(kind)),
      },
      {
        text: heading,
        tone: 'text',
        priority: 40,
        shrink: 'clip',
        clip,
        minWidth: 8,
      },
      ...status.map((segment, index) => ({
        ...segment,
        separatorBefore:
          segment.separatorBefore ?? (index === 0 ? '  ' : ' · '),
      })),
    ],
    end: secondary,
  };
}

/** Composition only: no tool/session state or syntax service ownership. */
export class ActivityCard extends VStack {
  private readonly row = new PriorityRow();
  private readonly notices = new Notices();
  // One detail frame per card; it reuses rows already framed at this width.
  // A row's call slot updates without a body before its result slot restores
  // it, so the frame survives detachment and still receives invalidation.
  private detail: DetailView | undefined;
  private detached = false;
  update(model: ActivityCardModel, theme: Theme): void {
    this.row.update(model.header, theme);
    this.notices.update(model.notices, theme);
    this.clear();
    const header: Component = {
      render: width =>
        this.row.render(width - Math.min(4, Math.floor(width / 10))),
      invalidate: () => this.row.invalidate(),
    };
    const decorated = new StateGutter(
      header,
      model.state ?? 'done',
      theme,
      model.expanded,
    );
    this.addChild(decorated);
    this.addChild(this.notices);
    if (model.body) {
      const caption =
        model.captionPolicy === 'when-not-in-header'
          ? (width: number) =>
              stripTerminalSequences(decorated.render(width).join('')).includes(
                labelText(model.caption),
              )
                ? ''
                : model.caption
          : model.caption;
      if (this.detail) this.detail.update(model.body, caption, theme);
      else this.detail = new DetailView(model.body, caption, theme);
      this.addChild(this.detail);
    }
    this.detached = !model.body;
  }
  override invalidate(): void {
    super.invalidate();
    if (this.detached) this.detail?.invalidate();
  }
  override render(width: number): string[] {
    return width > 0 ? super.render(width) : [];
  }
}
