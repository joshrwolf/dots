import { displayLimits } from "./policy.ts";
import { stripTerminalSequences, truncateToWidth, wrapTextWithAnsi } from "@earendil-works/pi-tui";

/** Raw records are never modified; these functions only prepare display text. */
export function sourceText(text: string): string {
  return stripTerminalSequences(text).replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u2069]/g, "").replaceAll("\t", "    ");
}
export function labelText(text: string): string { return sourceText(text).replace(/\s+/g, " ").trim(); }
export function statusText(text: string): string {
  return text.split(/(\x1b\[[\d;]*m)/).map((part) => /^\x1b\[[\d;]*m$/.test(part)
    ? part : stripTerminalSequences(part).replace(/[\x00-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u2069]/g, " ")).join("");
}

function* logicalLines(text: string): Generator<string> {
  let start = 0;
  for (;;) {
    const end = text.indexOf("\n", start);
    if (end < 0) { yield text.slice(start); return; }
    yield text.slice(start, end);
    start = end + 1;
  }
}

/** Limit work before wrapping: hostile single lines cannot allocate unlimited rows. */
export function layoutText(text: string, width: number, maxRows: number = displayLimits.previewRows): { lines: string[]; limited: boolean } {
  if (width <= 0 || maxRows <= 0) return { lines: [], limited: text.length > 0 };
  const lines: string[] = [];
  const safe = sourceText(text.slice(0, displayLimits.sourceCodeUnits));
  let omitted = text.length > displayLimits.sourceCodeUnits;
  for (const line of logicalLines(safe)) {
    const remaining = maxRows - lines.length;
    if (remaining <= 0) { omitted = true; break; }
    // Four UTF-16 units per cell is a conservative preparation budget, not a
    // width measurement. Pi owns actual grapheme/ANSI-aware layout.
    const budget = Math.max(64, width * remaining * 4);
    const chunk = line.slice(0, budget);
    const wrapped = wrapTextWithAnsi(chunk, width);
    lines.push(...wrapped.slice(0, remaining).map((part) => truncateToWidth(part, width, "")));
    if (chunk.length < line.length || wrapped.length > remaining) { omitted = true; break; }
  }
  if (omitted) {
    if (lines.length >= maxRows) lines.pop();
    lines.push(truncateToWidth("… display limited; inspect the recorded result for the remainder", width));
  }
  return { lines, limited: omitted };
}

export function plainLines(text: string, width: number, maxRows: number = displayLimits.previewRows): string[] {
  return layoutText(text, width, maxRows).lines;
}
