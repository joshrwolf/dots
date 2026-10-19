import { mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { randomUUID } from "node:crypto";
import { dirname, join } from "node:path";
import { getAgentDir } from "@earendil-works/pi-coding-agent";

export const toolNames = ["read", "write", "edit", "bash", "codemode"] as const;
export type ToolName = typeof toolNames[number];
export interface Preferences { readonly version: 1; readonly tools: readonly ToolName[]; readonly otherTools: "native" | "compact"; readonly header: "native" | "compact"; readonly footer: "native" | "compact"; readonly editor: "native" | "soft" }
export const defaultPreferences: Preferences = { version: 1, tools: toolNames, otherTools: "compact", header: "compact", footer: "compact", editor: "soft" };

export function parsePreferences(value: unknown): Preferences {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Expected an object");
  const fields: Record<string, unknown> = value as Record<string, unknown>;
  if (Object.keys(fields).some((key) => !["version", "tools", "otherTools", "header", "footer", "editor"].includes(key))) throw new Error("Unknown preference; expected version, tools, otherTools, header, footer, editor");
  if (fields.version !== 1) throw new Error("version must be 1");
  const tools = fields.tools === undefined ? toolNames : fields.tools;
  const otherTools = fields.otherTools === undefined ? Array.isArray(tools) && tools.length === 0 ? "native" : "compact" : fields.otherTools;
  if (otherTools !== "native" && otherTools !== "compact") throw new Error("otherTools must be native or compact");
  const header = fields.header === undefined ? "compact" : fields.header;
  if (header !== "native" && header !== "compact") throw new Error("header must be native or compact");
  const footer = fields.footer === undefined ? "compact" : fields.footer;
  if (!Array.isArray(tools) || !tools.every((name: unknown): name is ToolName => toolNames.some((known) => known === name)) || new Set(tools).size !== tools.length) throw new Error("tools must be a unique list of read, write, edit, bash, codemode");
  if (footer !== "native" && footer !== "compact") throw new Error("footer must be native or compact");
  const editor = fields.editor === undefined ? "soft" : fields.editor;
  if (editor !== "native" && editor !== "soft") throw new Error("editor must be native or soft");
  return { version: 1, tools: [...tools], otherTools, header, footer, editor };
}
export function loadPreferences(path = join(getAgentDir(), "dots-ui.json")): Preferences {
  let raw: string;
  try { raw = readFileSync(path, "utf8"); }
  catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return defaultPreferences;
    throw error;
  }
  try { return parsePreferences(JSON.parse(raw) as unknown); }
  catch (error) { throw new Error(`Invalid ${path}: ${error instanceof Error ? error.message : String(error)}`, { cause: error }); }
}

/** Atomic file replacement with best-effort intervening-edit detection.
 * This is not a cross-process compare-and-swap: simultaneous applies are
 * last-writer-wins. Avoid holding a process lock across a human interaction. */
export function savePreferences(next: Preferences, expected: Preferences, path = join(getAgentDir(), "dots-ui.json")): void {
  const value = parsePreferences(next);
  if (JSON.stringify(loadPreferences(path)) !== JSON.stringify(parsePreferences(expected))) throw new Error("UI preferences changed while the menu was open. Reopen /ui before saving.");
  mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
  const temporary = `${path}.${randomUUID()}.tmp`;
  try {
    writeFileSync(temporary, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600, flag: "wx" });
    renameSync(temporary, path);
  } finally { rmSync(temporary, { force: true }); }
}
