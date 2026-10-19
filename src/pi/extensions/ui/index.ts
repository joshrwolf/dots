import type { ExtensionAPI, ToolRenderers } from "@earendil-works/pi-coding-agent";
import { createToolRenderers, selectToolRenderers } from "../../src/host/tools.ts";
import { defaultPreferences, loadPreferences, type Preferences, type ToolName } from "../../src/host/preferences.ts";
import { createSyntaxService, type SyntaxService } from "../../src/ui/previews/syntax.ts";
import { codemodePresentation } from "../../src/features/tools/codemode.ts";
import { bashPresentation } from "../../src/features/tools/bash.ts";
import { readPresentation } from "../../src/features/tools/read.ts";
import { editPresentation } from "../../src/features/tools/edit.ts";
import { writePresentation } from "../../src/features/tools/write.ts";
import { configureUI } from "../../src/features/settings.ts";
import { installEditor } from "../../src/features/editor.ts";
import { createFooter } from "../../src/features/footer.ts";

export function toolRenderers(syntax: SyntaxService): ReadonlyMap<ToolName, ToolRenderers> {
  return new Map<ToolName, ToolRenderers>([
    ["codemode", createToolRenderers(codemodePresentation(syntax))],
    ["bash", createToolRenderers(bashPresentation(syntax))],
    ["read", createToolRenderers(readPresentation(syntax))],
    ["edit", createToolRenderers(editPresentation(syntax))],
    ["write", createToolRenderers(writePresentation(syntax))],
  ]);
}

export function registerUI(pi: ExtensionAPI, preferences: Preferences = defaultPreferences): void {
  // Rows can render before session_start. Creating the service is inert; its
  // first expanded source preview lazily acquires the highlighter.
  const syntax = createSyntaxService();
  const selected = new Map([...toolRenderers(syntax)].filter(([name]) => preferences.tools.includes(name)));
  pi.registerToolRenderer(selectToolRenderers(selected));
  const interactions = new Set<AbortController>();
  pi.registerCommand("ui", {
    description: "Configure tool displays, footer, and editor; apply with a reload",
    handler: async (_args, ctx) => {
      if (ctx.mode !== "tui") { ctx.ui.notify("/ui requires the interactive terminal.", "warning"); return; }
      const controller = new AbortController();
      interactions.add(controller);
      try { await configureUI(ctx.ui, () => ctx.reload(), undefined, controller.signal); }
      finally { interactions.delete(controller); }
    },
  });
  let releaseEditor = () => {};
  pi.on("session_shutdown", () => {
    for (const controller of interactions) controller.abort();
    interactions.clear();
    releaseEditor();
    syntax.dispose();
  });
  pi.on("session_start", (_event, ctx) => {
    releaseEditor();
    releaseEditor = preferences.editor === "soft" ? installEditor(ctx) : () => {};
    if (ctx.mode === "tui" && preferences.footer === "compact") ctx.ui.setFooter((tui, theme, data) => createFooter(ctx, theme, data, () => tui.requestRender()));
  });
}
export default function register(pi: ExtensionAPI): void { registerUI(pi, loadPreferences()); }
