import { CustomEditor, type CustomEditorOptions, type ExtensionContext, type KeybindingsManager } from "@earendil-works/pi-coding-agent";
import { styleTextWithAnsi, type EditorTheme, type TUI, type TuiMouseEvent } from "@earendil-works/pi-tui";

/** Native editing, focus, shortcuts, scroll hints, and working status; quieter chrome. */
export class SoftEditor extends CustomEditor {
  constructor(tui: TUI, theme: EditorTheme, keybindings: KeybindingsManager, options: CustomEditorOptions = {}) {
    super(tui, theme, keybindings, { paddingX: 1, embedWorkingStatus: true, ...options });
  }
  private minimumWidth(): number { return Math.max(3, this.getPaddingX() * 2 + 2); }
  override handleMouse(event: TuiMouseEvent) {
    if (event.width < this.minimumWidth()) return undefined;
    return super.handleMouse(event);
  }
  override render(width: number): string[] {
    if (width <= 0) return [];
    // Pi may change this callback for thinking level or bash mode. Soften the
    // current callback only during rendering; do not replace its semantic hue.
    const border = this.borderColor;
    this.borderColor = (text) => styleTextWithAnsi(border(text), undefined, undefined, { dim: true });
    try {
      // Native word wrapping needs room for a wide grapheme. Keep the draft
      // intact while a resize leaves less than that plus the desired padding.
      if (width < this.minimumWidth()) return [this.borderColor("…")];
      return super.render(width);
    }
    finally { this.borderColor = border; }
  }
}

/** Never take over another extension's editor or reset a later owner's factory. */
export function installEditor(ctx: Pick<ExtensionContext, "mode"> & { ui: Pick<ExtensionContext["ui"], "getEditorComponent" | "setEditorComponent"> }): () => void {
  if (ctx.mode !== "tui" || ctx.ui.getEditorComponent() !== undefined) return () => {};
  const factory: NonNullable<Parameters<ExtensionContext["ui"]["setEditorComponent"]>[0]> = (tui, theme, keybindings) => new SoftEditor(tui, theme, keybindings);
  ctx.ui.setEditorComponent(factory);
  let disposed = false;
  return () => {
    if (disposed) return;
    disposed = true;
    if (ctx.ui.getEditorComponent() === factory) ctx.ui.setEditorComponent(undefined);
  };
}
