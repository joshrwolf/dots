import type { Theme } from "@earendil-works/pi-coding-agent";
import { truncateToWidth, type Component } from "@earendil-works/pi-tui";
import { plainLines } from "./text.ts";

/** Shared expanded-detail treatment; children still own content and layout. */
export class DetailView implements Component {
  private readonly child: Component;
  private readonly caption: string | ((width: number) => string);
  private readonly theme: Theme;
  constructor(child: Component, caption: string | ((width: number) => string), theme: Theme) {
    this.child = child; this.caption = caption; this.theme = theme;
  }
  invalidate(): void { this.child.invalidate(); }
  render(width: number): string[] {
    if (width <= 0) return [];
    const inset = width >= 8 ? 2 : 0;
    const room = width - inset;
    const caption = typeof this.caption === "function" ? this.caption(width) : this.caption;
    const title = caption ? plainLines(caption, room, 8).map((line) => this.theme.fg("dim", line)) : [];
    const content = this.child.render(room);
    const lines = [...title, ...(content.length ? content : [this.theme.fg("dim", "(no content)")])];
    const rail = inset ? this.theme.fg("borderMuted", "│ ") : "";
    return lines.map((line) => rail + truncateToWidth(line, room));
  }
}
