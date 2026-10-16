import { existsSync, readFileSync } from "node:fs";

// A compact view of what a worker has been doing, read from its session file.
// Pi's JSONL session format is the contract: message entries carry assistant
// content blocks (text, toolCall) and toolResult content.

interface Block {
  type: string;
  text?: string;
  name?: string;
  arguments?: unknown;
}

const clean = (text: string) => text.replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, " ").trim();

export function transcriptTail(sessionFile: string | undefined, limit = 20, width = 160): string[] {
  if (!sessionFile || !existsSync(sessionFile)) return [];
  const lines: string[] = [];
  for (const raw of readFileSync(sessionFile, "utf8").split("\n")) {
    if (!raw) continue;
    let entry: { type?: string; message?: { role?: string; content?: unknown } };
    try {
      entry = JSON.parse(raw);
    } catch {
      continue;
    }
    const message = entry.message;
    if (!message) continue;
    const content = Array.isArray(message.content)
      ? (message.content as Block[])
      : typeof message.content === "string" ? [{ type: "text", text: message.content }] : [];
    if (message.role === "assistant") {
      for (const block of content) {
        if (block.type === "toolCall") {
          const args = block.arguments ? JSON.stringify(block.arguments) : "";
          lines.push(`-> ${block.name} ${clean(args)}`.slice(0, width));
        } else if (block.type === "text" && block.text?.trim()) {
          lines.push(`${clean(block.text).slice(0, width)}`);
        }
      }
    } else if (message.role === "toolResult") {
      const text = content.filter((block) => block.type === "text").map((block) => block.text ?? "").join(" ");
      if (text.trim()) lines.push(`   <- ${clean(text).slice(0, width)}`);
    }
  }
  const count = Math.max(0, Math.floor(limit));
  return count === 0 ? [] : lines.slice(-count);
}
