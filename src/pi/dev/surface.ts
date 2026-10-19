import { readFileSync } from "node:fs";
import type { Theme } from "@earendil-works/pi-coding-agent";
import { backgroundAnsi, parseColor, visibleWidth } from "@earendil-works/pi-tui";

// Development-only palette data, loaded once, never during rendering. The preview
// page is distinct from userMessageBg; the latter belongs only to prompts.
const pages = Object.fromEntries(["light", "dark"].map((appearance) => {
  const palette = JSON.parse(readFileSync(new URL(`../themes/dots-${appearance}.json`, import.meta.url), "utf8"));
  return [appearance, parseColor(palette.export.pageBg)];
}));

/** Rebind terminal-default SGR resets to the preview page, while preserving
 * explicit child colors (prompt panels, selections, diffs, syntax). This only
 * decorates existing lines; Pi still owns terminal rendering and composition. */
export function paintPreviewLine(line: string, width: number, theme: Theme): string {
  const fg = theme.getFgAnsi("text");
  const bg = backgroundAnsi(pages[theme.appearance]!, theme.getColorMode());
  const content = line.replace(/\x1b\[([\d;:]*)m/g, (sequence, parameters: string) => {
    const codes = parameters.split(";");
    let foreground = false, background = false;
    for (let i = 0; i < codes.length; i++) {
      const code = Number(codes[i]!.split(":")[0]);
      if (code === 0) { foreground = true; background = true; }
      else if (code === 39) foreground = true;
      else if (code === 49) background = true;
      else if (code === 38 || code >= 30 && code <= 37 || code >= 90 && code <= 97) foreground = false;
      else if (code === 48 || code >= 40 && code <= 47 || code >= 100 && code <= 107) background = false;
      // RGB/index payload values are not SGR commands. Colon-form payloads
      // already occupy one field and need no skipping.
      if ((code === 38 || code === 48 || code === 58) && !codes[i]!.includes(":")) {
        if (codes[i + 1] === "2") i += 4;
        else if (codes[i + 1] === "5") i += 2;
      }
    }
    return sequence + (foreground ? fg : "") + (background ? bg : "");
  });
  return fg + bg + content + bg + " ".repeat(Math.max(0, width - visibleWidth(line))) + "\x1b[0m";
}
