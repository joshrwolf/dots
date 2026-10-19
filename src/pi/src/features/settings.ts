import type { ExtensionContext } from "@earendil-works/pi-coding-agent";
import { defaultPreferences, loadPreferences, savePreferences, toolNames, type Preferences } from "../host/preferences.ts";

type SettingsUI = Pick<ExtensionContext["ui"], "select" | "notify">;

/** Native dialogs only. Draft changes stay local until Apply; ownership changes require reload. */
export async function configureUI(ui: SettingsUI, reload: () => Promise<void>, store = { load: loadPreferences, save: savePreferences }, signal?: AbortSignal): Promise<void> {
  if (signal?.aborted) return;
  try {
    const initial = store.load();
    let draft: Preferences = initial;
    for (;;) {
      const tools = toolNames.map((name) => `${name}: ${draft.tools.includes(name) ? "dots" : "native"}`);
      const footer = `footer: ${draft.footer}`;
      const editor = `editor: ${draft.editor}`;
      const selected = await ui.select("Presentation · choose a setting to toggle", [...tools, footer, editor, "Use dots UI", "Use native UI", "Apply and reload", "Cancel"], signal ? { signal } : undefined);
      if (signal?.aborted) return;
      if (!selected || selected === "Cancel") return;
      if (selected === "Apply and reload") { store.save(draft, initial); break; }
      if (selected === "Use dots UI") { draft = defaultPreferences; continue; }
      if (selected === "Use native UI") { draft = { version: 1, tools: [], footer: "native", editor: "native" }; continue; }
      if (selected === footer) { draft = { ...draft, footer: draft.footer === "compact" ? "native" : "compact" }; continue; }
      if (selected === editor) { draft = { ...draft, editor: draft.editor === "soft" ? "native" : "soft" }; continue; }
      const name = toolNames.find((_, index) => tools[index] === selected);
      if (name) draft = { ...draft, tools: toolNames.filter((tool) => tool === name ? !draft.tools.includes(tool) : draft.tools.includes(tool)) };
    }
  } catch (error) {
    if (!signal?.aborted) ui.notify(error instanceof Error ? error.message : String(error), "error");
    return;
  }
  // Reload may invalidate the UI even if replacement subsequently fails.
  // Let the host report its failure; never notify through the captured UI.
  if (!signal?.aborted) await reload();
}
