import type { Theme } from "@earendil-works/pi-coding-agent";
import { truncateToWidth, type Component } from "@earendil-works/pi-tui";

export type ActivityState = "receiving" | "queued" | "running" | "done" | "failed" | "cancelled" | "unknown";
const glyphs: Record<ActivityState, string> = { receiving: "·", queued: "◦", running: "◌", done: " ", failed: "✗", cancelled: "−", unknown: "?" };

/** Static state decoration. No timers, execution inference, or visibility assumptions. */
export class StateGutter implements Component {
  private readonly child: Component;
  private readonly state: ActivityState;
  private readonly theme: Theme;
  constructor(child: Component, state: ActivityState, theme: Theme) {
    this.child = child; this.state = state; this.theme = theme;
  }
  invalidate(): void { this.child.invalidate(); }
  render(width: number): string[] {
    if (width <= 0) return [];
    const tone = this.state === "failed" ? "error" : this.state === "running" ? "warning" : "muted";
    const prefix = this.theme.fg(tone, `${glyphs[this.state]} `);
    if (width <= 2) return [truncateToWidth(prefix, width)];
    return this.child.render(width - 2).map((line, index) => (index === 0 ? prefix : "  ") + line);
  }
}
