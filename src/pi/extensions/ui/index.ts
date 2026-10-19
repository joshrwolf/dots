import type { ExtensionAPI, ExtensionContext, ToolRenderers, ToolRendererResolver } from "@earendil-works/pi-coding-agent";
import { createToolRenderers } from "../../src/host/tools.ts";
import { defaultPreferences, loadPreferences, toolNames, type Preferences, type ToolName } from "../../src/host/preferences.ts";
import { createSyntaxService, type SyntaxService } from "../../src/ui/previews/syntax.ts";
import { codemodePresentation } from "../../src/features/tools/codemode.ts";
import { bashPresentation } from "../../src/features/tools/bash.ts";
import { readPresentation } from "../../src/features/tools/read.ts";
import { editPresentation } from "../../src/features/tools/edit.ts";
import { writePresentation } from "../../src/features/tools/write.ts";
import { activityPresentation } from "../../src/features/tools/activity.ts";
import { configureUI } from "../../src/features/settings.ts";
import { installEditor } from "../../src/features/editor.ts";
import { createFooter } from "../../src/features/footer.ts";
import { releaseStatus, releaseDescription } from "../../src/host/release.ts";
import { createHeader } from "../../src/features/frame.ts";

export function toolRenderers(syntax: SyntaxService): ReadonlyMap<ToolName, ToolRenderers> {
  return new Map<ToolName, ToolRenderers>([
    ["codemode", createToolRenderers(codemodePresentation(syntax))],
    ["bash", createToolRenderers(bashPresentation(syntax))],
    ["read", createToolRenderers(readPresentation(syntax))],
    ["edit", createToolRenderers(editPresentation(syntax))],
    ["write", createToolRenderers(writePresentation(syntax))],
  ]);
}

/** The workbench and live registration share this exact resolution path. */
export function rendererResolver(syntax: SyntaxService, preferences: Preferences = defaultPreferences): ToolRendererResolver {
  const selected = new Map([...toolRenderers(syntax)].filter(([name]) => preferences.tools.includes(name)));
  const other = new Map<string, ToolRenderers>();
  return (name, next) => {
    const owned = selected.get(name as ToolName);
    if (owned) return owned;
    const downstream = next();
    // Explicit native choices and third-party custom renderers retain ownership.
    // Only otherwise-unstyled tools get the shared transcript shell.
    if (toolNames.some((tool) => tool === name) || preferences.otherTools === "native" || downstream?.renderShell === "self" || downstream?.renderCall || downstream?.renderResult) return downstream;
    let renderer = other.get(name);
    if (!renderer) { renderer = createToolRenderers(activityPresentation(name)); other.set(name, renderer); }
    return renderer;
  };
}

export function registerUI(pi: ExtensionAPI, preferences: Preferences = defaultPreferences): void {
  // Rows can render before session_start. Syntax loading remains lazy.
  const syntax = createSyntaxService();
  pi.registerToolRenderer(rendererResolver(syntax, preferences));
  const interactions = new Set<AbortController>();
  const refreshRelease = (ctx: ExtensionContext) => {
    const status = releaseStatus();
    if (ctx.mode === "tui" && ctx.hasUI) ctx.ui.setStatus("dots-ui-release", status.reloadNeeded ? "Companion update · /reload when idle" : undefined);
    return status;
  };
  pi.registerCommand("ui-version", {
    description: "Show loaded and installed companion versions; does not install or reload",
    handler: async (_args, ctx) => { ctx.ui.notify(releaseDescription(refreshRelease(ctx)), "info"); },
  });
  pi.on("before_agent_start", (_event, ctx) => { refreshRelease(ctx); });
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
    refreshRelease(ctx);
    releaseEditor();
    releaseEditor = preferences.editor === "soft" ? installEditor(ctx) : () => {};
    if (ctx.mode === "tui" && preferences.header === "compact") ctx.ui.setHeader((_tui, theme) => createHeader(theme));
    if (ctx.mode === "tui" && preferences.footer === "compact") ctx.ui.setFooter((tui, theme, data) => createFooter(ctx, theme, data, () => tui.requestRender()));
  });
}
export default function register(pi: ExtensionAPI): void { registerUI(pi, loadPreferences()); }
