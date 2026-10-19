import { getMarkdownTheme, type Theme } from "@earendil-works/pi-coding-agent";
import { Markdown, truncateToWidth, type Component } from "@earendil-works/pi-tui";
import { sourceText, plainLines } from "./text.ts";
import { displayLimits } from "./policy.ts";

/** Human prose only; recorded tool output remains plain text unless its owner
 * knows it is Markdown. Bounds apply to preparation as well as rendered rows. */
export class MarkdownView implements Component {
  private readonly markdown: Markdown;
  private readonly limited: boolean;
  private readonly rows: number;
  private readonly theme: Theme;
  constructor(text: string, theme: Theme, rows: number = displayLimits.outputRows) {
    const bounded = sourceText(text.slice(0, 24_576));
    const lines = bounded.split("\n");
    this.limited = text.length > 24_576 || lines.length > 400;
    this.markdown = new Markdown(lines.slice(0, 400).join("\n"), 0, 0, getMarkdownTheme());
    this.rows = rows; this.theme = theme;
  }
  invalidate(): void { this.markdown.invalidate(); }
  render(width: number): string[] {
    if (width <= 0) return [];
    const lines = this.markdown.render(width).map((line) => truncateToWidth(line, width, ""));
    const limited = this.limited || lines.length > this.rows;
    return [...lines.slice(0, this.rows), ...(limited ? plainLines(this.rows <= 3 ? "… Ctrl+O to expand" : "… display limited; inspect the original document", width, 1).map((line) => this.theme.fg("dim", line)) : [])];
  }
}
