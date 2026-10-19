import { closeSync, fstatSync, openSync, readSync } from "node:fs";

// A bounded observation of recorded JSONL, never a second transcript parser.
// Skip a partial leading record; the complete history remains in the session.
const MAX_TAIL_BYTES = 1024 * 1024;
const MAX_LINES = 100;
const clean = (text: string) => text.replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, " ").trim();
const object = (value: unknown): Record<string, unknown> | undefined => value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;

export function transcriptTail(sessionFile: string | undefined, limit = 20, width = 160): string[] {
  const count = Number.isFinite(limit) ? Math.min(MAX_LINES, Math.max(0, Math.floor(limit))) : 20;
  if (!sessionFile || count === 0) return [];
  const columns = Number.isFinite(width) ? Math.min(1000, Math.max(1, Math.floor(width))) : 160;
  let fd: number | undefined;
  let source: string;
  let omitted = false;
  try {
    fd = openSync(sessionFile, "r");
    const size = fstatSync(fd).size;
    const start = Math.max(0, size - MAX_TAIL_BYTES);
    const bytes = Buffer.alloc(Math.min(size, MAX_TAIL_BYTES));
    const read = readSync(fd, bytes, 0, bytes.length, start);
    source = bytes.subarray(0, read).toString("utf8");
    if (start > 0) {
      omitted = true;
      const newline = source.indexOf("\n");
      source = newline < 0 ? "" : source.slice(newline + 1);
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
    return [`(transcript unavailable: ${clean((error as Error).message).slice(0, columns)})`];
  } finally { if (fd !== undefined) closeSync(fd); }
  const lines: string[] = [];
  for (const raw of source.split("\n")) {
    if (!raw) continue;
    let entry: Record<string, unknown> | undefined;
    try { entry = object(JSON.parse(raw)); } catch { continue; }
    const message = object(entry?.message);
    if (!message) continue;
    const content = Array.isArray(message.content) ? message.content.map(object).filter((block) => block !== undefined)
      : typeof message.content === "string" ? [{ type: "text", text: message.content }] : [];
    if (message.role === "assistant") {
      for (const block of content) {
        if (block.type === "toolCall") {
          const args = block.arguments ? JSON.stringify(block.arguments) : "";
          lines.push(clean(`-> ${String(block.name ?? "tool")} ${args}`).slice(0, columns));
        } else if (block.type === "text" && typeof block.text === "string" && block.text.trim()) {
          lines.push(clean(block.text).slice(0, columns));
        }
      }
    } else if (message.role === "toolResult") {
      const text = content.filter((block) => block.type === "text" && typeof block.text === "string").map((block) => block.text).join(" ");
      if (text.trim()) lines.push(`   <- ${clean(text)}`.slice(0, columns));
    }
    if (lines.length > MAX_LINES) lines.splice(0, lines.length - MAX_LINES);
  }
  if (omitted && lines.length < count) lines.unshift("(older or oversized records omitted; inspect the session file)");
  return lines.slice(-count);
}
