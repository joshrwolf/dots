import assert from "node:assert/strict";
import test from "node:test";
import { initTheme } from "@earendil-works/pi-coding-agent";
import { Text, stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { Panel, type PanelModel } from "@joshrwolf/pi-dots/ui";
import { FollowedWidget, GoalWidget, goalOverview } from "../extensions/goal/dashboard.ts";
import { GoalInspector, orientation } from "../extensions/goal/present.ts";
import { goalFixtures } from "../dev/goals.ts";
// Pinned test harness only; production uses the supplied host theme.
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";

initTheme("dark", false);
const plain = (lines: string[]) => stripTerminalSequences(lines.join("\n"));

test("panel framing stays cell-safe, theme-owned and subordinate to content", () => {
  const panel = new Panel();
  const model: PanelModel = { heading: { layout: "inline", start: [
    { text: "Goal café 日本語 👩🏽‍💻\x1b]52;c;payload\x07", tone: "accent", priority: 100, shrink: "clip", bold: true },
  ], end: [] }, body: new Text("A recorded task\nAnother task", 0, 0),
    footer: { start: [{ text: "Recorded progress", tone: "dim", priority: 20, shrink: "omit" }], end: [{ text: "inspect · /goal", tone: "muted", priority: 100, shrink: "clip" }] },
  };
  const rendered: string[][] = [];
  for (const name of ["dark", "light"]) {
    panel.update(model, getThemeByName(name)!);
    for (const width of [0, 1, 2, 8, 11, 12, 13, 20, 40, 80, 120, 240, 400]) {
      const lines = panel.render(width);
      assert.ok(lines.every((line) => visibleWidth(line) <= width));
      const clean = lines.map(stripTerminalSequences);
      assert.doesNotMatch(clean.join("\n"), /\x1b|\x07|payload|\uFFFD/);
      if (width >= 12) {
        assert.ok(clean[0]?.startsWith("╭")); assert.ok(clean[0]?.endsWith("╮"));
        assert.ok(clean.at(-1)?.startsWith("╰")); assert.ok(clean.at(-1)?.endsWith("╯"));
        assert.ok(lines.every((line) => visibleWidth(line) === width));
      } else assert.doesNotMatch(clean.join("\n"), /[╭╮╰╯│]/);
    }
    rendered.push(panel.render(80));
  }
  assert.equal(plain(rendered[0]!), plain(rendered[1]!));
  assert.notDeepEqual(rendered[0], rendered[1]);
  assert.match(plain(rendered[0]!), /A recorded task/);
});

test("dashboards retain recorded facts and original context across states, themes and widths", () => {
  for (let phase = 0; phase < 8; phase++) {
    const views = goalFixtures(phase);
    const original = structuredClone(views);
    const context = views.map((view) => orientation(view));
    for (const name of ["dark", "light"]) {
      const theme = getThemeByName(name)!;
      const widget = new FollowedWidget(); widget.update(views, views[0]!.focus.id, theme);
      for (const component of [widget, goalOverview(views[0]!, theme)]) {
        for (const width of [0, 1, 2, 12, 30, 60, 120, 240, 400]) {
          const lines = component.render(width);
          assert.ok(lines.length < 30);
          assert.ok(lines.every((line) => visibleWidth(line) <= width), `${phase}/${name}/${width}`);
          assert.doesNotMatch(plain(lines), /\uFFFD|\x1b|\x07/);
        }
        const result = plain(component.render(120));
        assert.doesNotMatch(result, /running|landed|Current:/i);
        if (phase === 0) assert.match(result, /✓ Done +Define shared types[\s\S]*› Claimed +Update exports[\s\S]*· Waiting +Adapt consumers/);
        if (phase === 1) assert.match(result, /! Held +Resolve compatibility/);
        if (phase === 2) assert.match(result, /4\/4 slices done/);
        if (phase === 4) { assert.match(result, /Unavailable/); assert.doesNotMatch(result, /slices done|Define shared|█/); }
        if (phase === 5) { assert.match(result, /Loading goal/); assert.doesNotMatch(result, /slices done|█/); }
        if (phase === 6) assert.match(result, /Brief truncated/);
        if (phase === 7) { assert.match(result, /No slices recorded/); assert.doesNotMatch(result, /0\/0|█/); }
      }
    }
    assert.deepEqual(views, original);
    assert.deepEqual(views.map((view) => orientation(view)), context);
  }
});

test("short previews favor claims and blockers, report omissions, and never elevate expired claims", () => {
  const first = goalFixtures()[0]!;
  const base = first.snapshot!;
  const prototype = base.overview.tasks[0]!;
  const tasks = Array.from({ length: 9 }, (_, i) => ({ ...prototype, id: `t-${i}`, title: `Recorded ${i}` }));
  tasks.push({ ...prototype, id: "t-999999", title: "Needs attention", status: "held" });
  tasks.push({ ...prototype, id: "t-aaaaaa", title: "Current claim", status: "open", owner: "fixture", lease_until: 101 });
  tasks.push({ ...prototype, id: "t-bbbbbb", title: "Expired claim", status: "open", owner: "fixture", lease_until: 99 });
  const model = { ...first, snapshot: { ...base, overview: { ...base.overview, tasks,
    progress: { total: 12, done: 9, open: 2, held: 1, claimed: 1, dropped: 0, ready: 1, with_pr: 0 },
  } } };
  const widget = new GoalWidget(); widget.update(model, getThemeByName("dark")!);
  const output = plain(widget.render(120));
  assert.match(output, /Held +Needs attention/);
  assert.match(output, /Claimed +Current claim/);
  assert.match(output, /Open +Expired claim/);
  assert.match(output, /8 more slices/);
  assert.doesNotMatch(output, /Claimed +Expired claim/);
});

test("inspection keeps the selected section through refresh and resets it for a different goal", () => {
  const goal = goalFixtures()[0]!;
  const theme = getThemeByName("dark")!;
  const inspector = new GoalInspector({ matches: () => false }, () => {}, () => {}, () => 20);
  inspector.setGoal(goal, theme, () => {});
  inspector.handleInput("2");
  assert.match(plain(inspector.render(120)), /Preserve compatibility/);
  inspector.setGoal(goal, getThemeByName("light")!, () => {});
  assert.match(plain(inspector.render(120)), /Preserve compatibility/);
  inspector.setGoal(goalFixtures(3)[1]!, theme, () => {});
  assert.doesNotMatch(plain(inspector.render(120)), /Preserve compatibility/);
  assert.match(plain(inspector.render(120)), /Slices/);
});
