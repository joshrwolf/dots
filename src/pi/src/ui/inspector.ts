import type { KeybindingsManager, Theme } from "@earendil-works/pi-coding-agent";
import { MouseRegion, ScrollView, VStack, matchesKey, type Component } from "@earendil-works/pi-tui";
import { PriorityRow } from "./row.ts";

export interface InspectorModel {
  readonly title: string;
  readonly detail?: string;
  readonly body: Component;
  readonly hint?: string;
}

/** Pi 1.3's editor slot and overlays render through a legacy Container rather
 * than the fullscreen layout walker. Supply explicit bounds there, using the
 * public ScrollView state API; never reach into the host's component tree. */
class InspectorViewport extends ScrollView {
  private readonly height: () => number;
  private readonly redraw: () => void;
  constructor(content: Component, height: () => number, redraw: () => void) {
    super(content, { scrollbar: "hidden", overscroll: "contain" });
    this.height = height; this.redraw = redraw;
  }
  override render(width: number): string[] {
    if (width <= 0) return [];
    const lines = super.render(width);
    this.updateLayout(lines.length, Math.min(lines.length, Math.max(1, Math.floor(this.height()))), this.redraw);
    return lines.slice(this.scrollTop, this.scrollTop + this.viewportHeight);
  }
}

/** Bounded read-only inspection. Pi owns terminal, focus and scheduling;
 * ScrollView owns scroll state. The caller owns actions and lifetime. */
export class Inspector extends VStack {
  private readonly heading = new PriorityRow();
  private readonly content = new VStack();
  private readonly viewport: InspectorViewport;
  private readonly hint = new PriorityRow();
  private readonly bindings: Pick<KeybindingsManager, "matches">;
  private readonly close: () => void;
  private readonly redraw: () => void;

  constructor(bindings: Pick<KeybindingsManager, "matches">, close: () => void, redraw: () => void, height: () => number = () => 20) {
    super();
    this.bindings = bindings; this.close = close; this.redraw = redraw;
    this.addChild(this.heading);
    this.viewport = new InspectorViewport(this.content, height, redraw);
    this.addChild(new MouseRegion(this.viewport, (event) => {
      if (event.type !== "wheel") return undefined;
      this.viewport.scrollBy(event.wheelDelta ?? 0);
      return { handled: true, render: true };
    }), { grow: 1, minSize: 1, maxSize: 32 });
    this.addChild(this.hint);
  }

  /** Reset scroll only when selecting a different document, not on live refresh. */
  setContent(model: InspectorModel, theme: Theme, resetScroll = false): void {
    this.heading.update({ layout: "inline", groupSeparator: "  ", start: [
      { text: model.title, tone: "toolTitle", bold: true, priority: 100, shrink: "clip", minWidth: 8 },
    ], end: [{ text: model.detail ?? "", tone: "dim", priority: 10, shrink: "omit" }] }, theme);
    this.content.clear();
    this.content.addChild(model.body);
    this.hint.update({ start: [{ text: model.hint ?? "↑/↓ PgUp/PgDn scroll · Enter/Esc close", tone: "dim", priority: 1, shrink: "clip" }], end: [] }, theme);
    if (resetScroll) this.viewport.scrollToStart();
    this.invalidate();
  }

  handleInput(data: string): void {
    if (this.bindings.matches(data, "tui.select.cancel") || this.bindings.matches(data, "tui.select.confirm")) { this.close(); return; }
    if (matchesKey(data, "up")) this.viewport.scrollBy(-1);
    else if (matchesKey(data, "down")) this.viewport.scrollBy(1);
    else if (matchesKey(data, "pageUp")) this.viewport.scrollBy(-Math.max(1, this.viewport.viewportHeight));
    else if (matchesKey(data, "pageDown")) this.viewport.scrollBy(Math.max(1, this.viewport.viewportHeight));
    else if (matchesKey(data, "home")) this.viewport.scrollToStart();
    else if (matchesKey(data, "end")) this.viewport.scrollToEnd();
    else return;
    this.redraw();
  }
}
