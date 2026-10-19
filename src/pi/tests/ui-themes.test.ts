import assert from "node:assert/strict";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { DefaultResourceLoader, SessionManager, SettingsManager, UserMessageComponent, type ThemeColor } from "@earendil-works/pi-coding-agent";
import { DiffView, createSyntaxService, parsePatch, readableForeground } from "@joshrwolf/pi-dots/ui";
import { colorToRgb, parseColor, stripTerminalSequences, visibleWidth, type Color } from "@earendil-works/pi-tui";
// Exact-version host-contract imports; themes require no runtime extension code.
import { validateThemeJson } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme-schema.js";
import { loadThemeFromPath, resolveThemeSetting, setRegisteredThemes, setThemeInstance } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { exportSessionToHtml } from "../node_modules/@earendil-works/pi-coding-agent/dist/core/export-html/index.js";

const root = fileURLToPath(new URL("../", import.meta.url));
const appearances = ["light", "dark"] as const;
function luminance(color: Color): number {
  const { r, g, b } = colorToRgb(color);
  const linear = (byte: number) => { const s = byte / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}
function contrast(a: Color, b: Color): number {
  const first = luminance(a), second = luminance(b);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}
const bodyRoles: readonly ThemeColor[] = ["accent", "text", "userMessageText", "customMessageText", "toolTitle", "toolOutput", "muted", "dim", "thinkingText", "success", "error", "warning", "mdLink", "mdCode", "syntaxComment", "syntaxKeyword", "syntaxFunction", "syntaxVariable", "syntaxString", "syntaxNumber", "syntaxType", "syntaxOperator", "syntaxPunctuation"];

for (const appearance of appearances) {
  test(`${appearance} palette validates, keeps quiet surfaces, and provides readable text`, async () => {
    const path = join(root, "themes", `dots-${appearance}.json`);
    const json = validateThemeJson(path, JSON.parse(await readFile(path, "utf8")) as unknown);
    assert.equal(json.name, `dots-${appearance}`);
    assert.equal(json.appearance, appearance);
    assert.equal(json.colors.toolPendingBg, "");
    assert.equal(json.colors.toolSuccessBg, "");
    assert.equal(json.colors.customMessageBg, "");
    const theme = loadThemeFromPath(path, "truecolor");
    assert.ok(json.export?.pageBg);
    const page = parseColor(json.export.pageBg);
    for (const background of [page, theme.colors.userMessageBg, theme.colors.toolErrorBg]) {
      for (const role of bodyRoles) assert.ok(contrast(theme.colors[role], background) >= 4.5, `${appearance}: ${role} contrast ${contrast(theme.colors[role], background).toFixed(2)}`);
    }
    // Decorative borders are deliberately quiet, not body-text contrast targets.
    assert.ok(contrast(theme.colors.borderMuted, page) < contrast(theme.colors.muted, page));
    assert.notDeepEqual(theme.colors.error, theme.colors.success);
    assert.notDeepEqual(theme.colors.userMessageBg, theme.colors.selectedBg);
    for (const mode of ["truecolor", "256color"] as const) {
      setThemeInstance(loadThemeFromPath(path, mode));
      const user = new UserMessageComponent("A quiet prompt with **emphasis**, `code`, and 日本語 👩🏽‍💻.");
      for (const width of [12, 40, 100]) {
        const lines = user.render(width);
        assert.ok(lines.every((line) => visibleWidth(line) <= width));
        assert.doesNotMatch(stripTerminalSequences(lines.join("\n")), /undefined|NaN/);
      }
    }
  });
}

test("contrast adjustment preserves already-readable colors and moves both light and dark foregrounds", () => {
  for (const [foreground, background] of [["#eeeeee", "#111111"], ["#777777", "#dddddd"], ["#555555", "#222222"]]) {
    assert.ok(foreground && background);
    const fg = parseColor(foreground), bg = parseColor(background);
    const result = readableForeground(fg, bg);
    assert.ok(contrast(result, bg) >= 4.5);
    if (contrast(fg, bg) >= 4.5) assert.deepEqual(result, fg);
    assert.deepEqual(readableForeground(fg, bg), result);
  }
});

test("highlighted diff words remain readable on both palettes", async () => {
  const syntax = createSyntaxService();
  try {
    const patch = parsePatch('@@ -1 +1 @@\n-const answer = 42; // previous\n+const answer = 43; // updated\n');
    assert.ok(patch);
    for (const appearance of appearances) {
      const theme = loadThemeFromPath(join(root, "themes", `dots-${appearance}.json`), "truecolor");
      const original = theme.style.bind(theme);
      const ratios: number[] = [];
      theme.style = (text, options) => {
        if (text.trim() && options.fg && options.bg) {
          const fg = typeof options.fg === "string" ? theme.colors[options.fg] : options.fg;
          const bg = typeof options.bg === "string" ? theme.colors[options.bg] : options.bg;
          ratios.push(contrast(fg, bg));
        }
        return original(text, options);
      };
      const view = new DiffView({ patch, path: "example.ts", theme, syntax });
      await view.prepare();
      for (const width of [40, 80, 140]) view.render(width);
      assert.ok(ratios.length > 0);
      assert.ok(Math.min(...ratios) >= 4.5, `${appearance} diff contrast: ${Math.min(...ratios).toFixed(2)}`);
    }
  } finally { syntax.dispose(); }
});

test("the package discovers both themes, automatic mode resolves each, and real exports use their surfaces", async () => {
  const dir = await mkdtemp(join(tmpdir(), "dots-theme-host-"));
  try {
    const loader = new DefaultResourceLoader({ cwd: dir, agentDir: dir, settingsManager: SettingsManager.inMemory({ packages: [root] }), noExtensions: true, noSkills: true, noPromptTemplates: true, noContextFiles: true });
    await loader.reload();
    const { themes, diagnostics } = loader.getThemes();
    assert.deepEqual(diagnostics, []);
    assert.deepEqual(themes.map((theme) => theme.name).sort(), ["dots-dark", "dots-light"]);
    setRegisteredThemes(themes);
    const session = SessionManager.create(dir, dir);
    session.appendMessage({ role: "user", content: "Hand-authored presentation fixture", timestamp: 1 });
    for (const appearance of appearances) {
      const name = `dots-${appearance}`;
      assert.equal(resolveThemeSetting("dots-light/dots-dark", appearance), name);
      const json = validateThemeJson(name, JSON.parse(await readFile(join(root, "themes", `${name}.json`), "utf8")) as unknown);
      const output = await exportSessionToHtml(session, undefined, { themeName: name, outputPath: join(dir, `${name}.html`) });
      const html = await readFile(output, "utf8");
      assert.ok(html.includes(`--body-bg: ${json.export?.pageBg};`));
      assert.ok(html.includes(`--container-bg: ${json.export?.cardBg};`));
      assert.ok(html.includes(`--info-bg: ${json.export?.infoBg};`));
    }
  } finally { setRegisteredThemes([]); await rm(dir, { recursive: true, force: true }); }
});
