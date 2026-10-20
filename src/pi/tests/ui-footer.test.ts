import assert from "node:assert/strict";
import test from "node:test";
import { initTheme } from "@earendil-works/pi-coding-agent";
import { stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { FooterView, type FooterModel } from "../src/features/frame.ts";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

initTheme("dark", false);
const model: FooterModel = { project: "example", branch: "feature/presentation", model: "example-model", thinking: "high", contextPercent: 25, statuses: [] };
const plain = (view: FooterView, width = 120) => stripTerminalSequences(view.render(width).join("\n"));

test("the default footer uses foreground colors and icons without bars or special fonts", () => {
  const view = new FooterView();
  for (const name of ["dark", "light"]) {
    const theme = getThemeByName(name)!;
    view.update(model, theme);
    const rendered = view.render(120);
    for (const value of ["example", "feature/presentation", "example-model", "high", "ctx 25%"]) assert.ok(plain(view).includes(value));
    assert.equal(rendered.length, 1);
    assert.doesNotMatch(rendered.join(""), /\x1b\[(?:4[0-7]|48[;:])|[\ue000-\uf8ff█▓▒░]/, "no colored backgrounds, bars or Nerd Font requirement");
    assert.match(plain(view), /⑂ feature\/presentation.*✦ high.*◔ ctx 25%/);
  }
});

test("the footer preserves unknown and warning states, sanitizes labels and stays bounded", () => {
  const view = new FooterView();
  for (const name of ["dark", "light"]) {
    const theme = getThemeByName(name)!;
    for (const percent of [undefined, NaN, 0, 80, 95]) {
      view.update({ ...model, project: "日本語 👩🏽‍💻\x1b[2J", branch: "feature/".repeat(20), contextPercent: percent,
        statuses: ["workers 2 · awaiting input", "publisher: " + "additional detail ".repeat(20)] }, theme);
      for (let width = 1; width <= 160; width++) {
        const rendered = view.render(width);
        assert.equal(rendered.length, 2);
        assert.ok(rendered.every((line) => visibleWidth(line) <= width));
        assert.doesNotMatch(rendered.join(""), /\x1b\[2J|\uFFFD/);
      }
      assert.match(plain(view, 20), percent === undefined || Number.isNaN(percent) ? /ctx \?/ : new RegExp(`ctx ${percent}%`));
      if (percent !== undefined && percent >= 75) {
        assert.match(plain(view), /! ctx/);
        assert.ok(view.render(120)[0]!.includes(theme.getFgAnsi(percent >= 90 ? "error" : "warning") + `ctx ${percent}%`));
      }
    }
    view.update({ project: "example", model: "no model", thinking: "off", statuses: [] }, theme);
    assert.match(plain(view), /no model.*ctx \?/);
    assert.doesNotMatch(plain(view), /think|✦|⑂|feature|0%/);
  }
});
