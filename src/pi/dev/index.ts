import type { OverlayHandle } from "@earendil-works/pi-tui";
import { getSelectListTheme, type ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { SoftEditor, createSyntaxService } from "../src/index.ts";
import { Workbench } from "./workbench.ts";

/** Explicit development resource; never listed in the distributable manifest. */
export default function workbenchExtension(pi: ExtensionAPI): void {
  if (process.env["DOTS_UI_WORKBENCH"] !== "1") throw new Error("Use mise run //src/pi:workbench for an isolated UI workbench.");
  let current: { close: (restoreTheme?: boolean) => void } | undefined;
  pi.on("input", () => { pi.events.emit("dots:workbench-input", undefined); return { action: "handled" }; }); // No provider turns.
  pi.on("session_start", (_event, ctx) => {
    if (ctx.mode === "tui") ctx.ui.setEditorText(`/ui-lab ${process.env["DOTS_UI_SCENARIO"] ?? "frame"}`);
  });
  pi.on("session_shutdown", () => { current?.close(false); current = undefined; });
  pi.registerCommand("ui-lab-exit", { handler: async (_args, ctx) => { ctx.shutdown(); } });
  pi.registerCommand("ui-lab", {
    description: "Fixture-only presentation workbench; choose a scenario with N/P",
    handler: async (args, ctx) => {
      if (ctx.mode !== "tui") { ctx.ui.notify("UI workbench requires TUI mode", "warning"); return; }
      if (current) { ctx.ui.notify("Workbench is already open", "warning"); return; }
      const initialTheme = ctx.ui.theme.name ?? `dots-${ctx.ui.theme.appearance}`;
      const syntax = createSyntaxService();
      let view: Workbench | undefined;
      let overlay: OverlayHandle | undefined;
      try {
        await ctx.ui.custom<void>((tui, _theme, bindings, done) => {
          let closed = false;
          const close = (restoreTheme = true) => {
            if (closed) return;
            closed = true;
            if (restoreTheme) ctx.ui.setTheme(initialTheme);
            view?.dispose(); done();
          };
          current = { close };
          view = new Workbench({ syntax, tui, bindings, close, redraw: () => tui.requestRender(), theme: () => ctx.ui.theme,
            height: () => Math.max(1, Math.min(24, tui.terminal.rows - 12)),
            createEditor: (theme) => {
              const editor = new SoftEditor(tui, { borderColor: (text) => theme.fg("thinkingHigh", text), selectList: getSelectListTheme() }, bindings);
              editor.setText("Review the result and suggest the next step.");
              return editor;
            },
            toggleTheme: () => {
              const next = ctx.ui.theme.appearance === "dark" ? "dots-light" : "dots-dark";
              const result = ctx.ui.setTheme(next);
              if (!result.success) throw new Error(result.error);
            },
            confirmStop: async (signal) => {
              overlay?.setHidden(true);
              let approved: boolean;
              try { approved = await ctx.ui.confirm("Simulate stopping this job?", "Fixture state only. No process is started or signalled.", { signal }); }
              finally { if (!signal.aborted) { overlay?.setHidden(false); overlay?.focus(); } }
              pi.events.emit("dots:workbench-confirmed", approved);
              return approved;
            },
            changed: (state) => pi.events.emit("dots:workbench", state),
          }, args.trim() || "frame");
          return view;
        }, { overlay: true, overlayOptions: { anchor: "top-left", margin: 0, width: "100%", maxHeight: "90%" }, onHandle: (handle) => { overlay = handle; } });
      } finally {
        view?.dispose(); syntax.dispose(); current = undefined;
        pi.events.emit("dots:workbench-closed", undefined);
      }
    },
  });
}
