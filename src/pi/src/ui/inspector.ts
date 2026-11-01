import type {
  ExtensionContext,
  KeybindingsManager,
  Theme,
} from '@earendil-works/pi-coding-agent';
import {
  MouseRegion,
  ScrollView,
  VStack,
  matchesKey,
  type Component,
} from '@earendil-works/pi-tui';
import {Panel, type RowModel} from '@joshrwolf/pi-kit/ui';

/** Shared, draft-preserving inspection bounds for persistent activity. */
export const inspectorOverlay = {
  overlay: true,
  overlayOptions: {
    anchor: 'center',
    width: '80%',
    minWidth: 60,
    maxHeight: '80%',
    margin: 1,
  },
} satisfies NonNullable<Parameters<ExtensionContext['ui']['custom']>[1]>;
/** Body rows: the overlay reserves two more lines for its heading and footer. */
export const inspectorHeight = (rows: number): number =>
  Math.max(1, Math.floor(rows * 0.8) - 2);

export interface InspectorModel {
  readonly title: string;
  readonly detail?: string;
  readonly body: Component;
  readonly hint?: string;
  /** One pinned row below the scrollable body, such as a focused Input. */
  readonly accessory?: Component;
  readonly footer?: RowModel;
}

/** Overlay rendering supplies width but not viewport height. Set explicit bounds
 * through the public ScrollView state API without inspecting the host's tree. */
class InspectorViewport extends ScrollView {
  private readonly height: () => number;
  private readonly redraw: () => void;
  private pendingLine: number | undefined;
  private pendingEnd = false;
  revealLine(line: number): void {
    this.pendingEnd = false;
    this.pendingLine = Math.max(0, Math.floor(line));
  }
  jumpToEnd(): void {
    this.pendingEnd = true;
    this.scrollToEnd();
  }
  constructor(
    content: Component,
    height: () => number,
    redraw: () => void,
    follow: 'none' | 'end',
  ) {
    super(content, {scrollbar: 'hidden', overscroll: 'contain', follow});
    this.height = height;
    this.redraw = redraw;
  }
  override render(width: number): string[] {
    if (width <= 0) return [];
    const lines = super.render(width);
    this.updateLayout(
      lines.length,
      Math.min(lines.length, Math.max(0, Math.floor(this.height()))),
      this.redraw,
    );
    if (this.pendingEnd) {
      this.pendingEnd = false;
      this.scrollToEnd();
    }
    if (this.pendingLine !== undefined) {
      const line = Math.min(this.pendingLine, Math.max(0, lines.length - 1));
      this.pendingLine = undefined;
      if (line < this.scrollTop) this.scrollTo(line);
      else if (line >= this.scrollTop + this.viewportHeight)
        this.scrollTo(line - this.viewportHeight + 1);
    }
    return lines.slice(this.scrollTop, this.scrollTop + this.viewportHeight);
  }
}

/** Bounded read-only inspection. Pi owns terminal, focus and scheduling;
 * ScrollView owns scroll state. The caller owns actions and lifetime. */
export class Inspector extends VStack {
  private readonly panel = new Panel();
  private readonly content = new VStack();
  private readonly body = new VStack();
  private readonly viewport: InspectorViewport;
  private accessory: Component | undefined;
  private readonly bindings: Pick<KeybindingsManager, 'matches'>;
  private readonly close: () => void;
  private readonly redraw: () => void;

  constructor(
    bindings: Pick<KeybindingsManager, 'matches'>,
    close: () => void,
    redraw: () => void,
    height: () => number = () => 20,
    follow: 'none' | 'end' = 'none',
  ) {
    super();
    this.bindings = bindings;
    this.close = close;
    this.redraw = redraw;
    this.viewport = new InspectorViewport(
      this.content,
      () => Math.max(0, height() - (this.accessory ? 1 : 0)),
      redraw,
      follow,
    );
    this.addChild(this.panel);
  }

  /** Reset scroll only when selecting a different document, not on live refresh. */
  setContent(model: InspectorModel, theme: Theme, resetScroll = false): void {
    this.content.clear();
    this.content.addChild(model.body);
    this.accessory = model.accessory;
    this.body.clear();
    this.body.addChild(
      new MouseRegion(this.viewport, event => {
        if (event.type !== 'wheel') return undefined;
        this.viewport.scrollBy(event.wheelDelta ?? 0);
        return {handled: true, render: true};
      }),
    );
    if (model.accessory) this.body.addChild(model.accessory);
    this.panel.update(
      {
        heading: {
          layout: 'inline',
          groupSeparator: '  ',
          start: [
            {
              text: model.title,
              tone: 'toolTitle',
              bold: true,
              priority: 100,
              shrink: 'clip',
              minWidth: 8,
            },
          ],
          end: [
            {
              text: model.detail ?? '',
              tone: 'dim',
              priority: 10,
              shrink: 'omit',
            },
          ],
        },
        body: this.body,
        footer: model.footer ?? {
          start: [
            {
              text: model.hint ?? '↑/↓ PgUp/PgDn scroll · Enter/Esc close',
              tone: 'dim',
              priority: 1,
              shrink: 'clip',
            },
          ],
          end: [],
        },
      },
      theme,
    );
    if (resetScroll) {
      this.viewport.scrollToStart();
      this.viewport.revealLine(0);
    }
    this.invalidate();
  }

  get isFollowingEnd(): boolean {
    return this.viewport.isFollowingEnd;
  }
  followEnd(): void {
    this.viewport.jumpToEnd();
    this.redraw();
  }

  /** Reveal a known body row after its new layout, without replacing native scrolling. */
  revealLine(line: number): void {
    this.viewport.revealLine(line);
    this.redraw();
  }

  handleInput(data: string): void {
    if (
      this.bindings.matches(data, 'tui.select.cancel') ||
      this.bindings.matches(data, 'tui.select.confirm')
    ) {
      this.close();
      return;
    }
    if (matchesKey(data, 'up')) this.viewport.scrollBy(-1);
    else if (matchesKey(data, 'down')) this.viewport.scrollBy(1);
    else if (matchesKey(data, 'pageUp'))
      this.viewport.scrollBy(-Math.max(1, this.viewport.viewportHeight));
    else if (matchesKey(data, 'pageDown'))
      this.viewport.scrollBy(Math.max(1, this.viewport.viewportHeight));
    else if (matchesKey(data, 'home')) this.viewport.scrollToStart();
    else if (matchesKey(data, 'end')) this.viewport.jumpToEnd();
    else return;
    this.redraw();
  }
}
