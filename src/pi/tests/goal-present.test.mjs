import assert from "node:assert/strict";
import test from "node:test";
import { initTheme } from "@earendil-works/pi-coding-agent";
import { stripTerminalSequences, visibleWidth } from "@earendil-works/pi-tui";
import { getThemeByName } from "../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js";
import { Inspector } from "../src/index.ts";
import { GoalWidget, FollowedWidget, GoalInspector, compactSummary, overview, inspection, orientation, setInspection } from "../extensions/goal/present.ts";
import { snapshot, task } from "./goal-fixture.mjs";
initTheme("dark", false);
const plain = (lines) => stripTerminalSequences(lines.join("\n"));

test("goal views use shared components, fit widths and sanitize terminal controls", () => {
  const value = snapshot("t-123abc", "cafe\u0301 日本語 👩‍💻 \x1b]52;c;payload\x07");
  const model = { focus: { id: "t-123abc", project: value.project }, snapshot: value,
    brief: { text: "# Pattern\n\x1b[2JUse a shared interface." } };
  for (const name of ["dark", "light"]) {
    const theme = getThemeByName(name), widget = new GoalWidget();
    widget.update(model, theme);
    let closes = 0;
    const inspector = new Inspector({ matches: (key, action) => key === "\r" && action === "tui.select.confirm" }, () => closes++, () => {}, () => 8);
    setInspection(inspector, model, theme);
    for (const width of [1, 2, 10, 40, 80, 160]) {
      for (const view of [widget, inspector]) {
        const lines = view.render(width);
        assert.ok(lines.every((line) => visibleWidth(line) <= width), `${name} ${width}`);
        assert.doesNotMatch(plain(lines), /\x1b|\x07|payload/);
      }
    }
    assert.doesNotMatch(plain(widget.render(160)), /focus only|0 held|0 dropped|PR recorded/);
    assert.match(plain(widget.render(160)), /0\/1 slices done/);
    inspector.handleInput("\r"); assert.equal(closes, 1);
  }
});

test("goal overview distinguishes claims, dependencies, held work and history without exposing bookkeeping", () => {
  const value = snapshot();
  value.overview.tasks = [
    task("t-aa", "Reference", { owner: "fixture", lease_until: 101 }),
    task("t-bb", "Consumer", { blocked_by: ["t-aa"] }),
    task("t-cc", "Decision", { status: "held" }),
    task("t-dd", "Expired", { owner: "fixture", lease_until: 99 }),
  ];
  value.overview.progress = { total: 5, open: 3, done: 1, held: 1, dropped: 0, claimed: 1, ready: 1, with_pr: 0 };
  value.overview.hidden_closed = 1;
  const model = { focus: { id: "t-123abc", project: value.project }, snapshot: value, brief: { text: "# Accepted design\n\nUse **one** interface." } };
  const theme = getThemeByName("dark"), widget = new GoalWidget();
  widget.update(model, theme);
  assert.match(plain(widget.render(160)), /1\/5 slices done · 1 claimed · 1 held.*Claimed: Reference/);
  assert.match(overview(model), /Waiting +Consumer\n +Needs: Reference/);
  assert.match(overview(model), /Open +Expired/);
  assert.doesNotMatch(overview(model), /Project:|Recorded at:|t-aa|Brief content:/);
  assert.match(overview(model), /closed task details hidden/);
  value.overview.progress.claimed = 2;
  widget.update(model, theme);
  assert.doesNotMatch(plain(widget.render(160)), /Claimed:/, "multiple claims must not invent a current slice");
  assert.equal(compactSummary({ ...model, error: "offline" }), "unavailable · /goal refresh");
  let redraws = 0;
  const inspector = new GoalInspector({ matches: () => false }, () => {}, () => {}, () => 20);
  inspector.setGoal(model, theme, () => redraws++);
  assert.doesNotMatch(plain(inspector.render(120)), /Accepted design|Project:/);
  inspector.handleInput("2");
  assert.match(plain(inspector.render(120)), /Accepted design/);
  assert.doesNotMatch(plain(inspector.render(120)), /\*\*one\*\*/);
  inspector.handleInput("3");
  assert.match(plain(inspector.render(120)), /Project:/);
  inspector.handleInput("1");
  assert.match(plain(inspector.render(120)), /Waiting +Consumer/);
  assert.equal(redraws, 3);
});

test("followed overview keeps selection visible and reports display omissions without hiding context", () => {
  const views = Array.from({ length: 8 }, (_, n) => {
    const value = snapshot(`t-${n.toString(16)}`, `Goal ${n}`);
    return { focus: { id: value.overview.goal.root.id, project: value.project }, snapshot: value, brief: { text: "日本語 👩‍💻" } };
  });
  views[0] = { focus: views[0].focus, error: "Unavailable" };
  for (const name of ["dark", "light"]) {
    const widget = new FollowedWidget(); widget.update(views, "t-7", getThemeByName(name));
    const output = plain(widget.render(160));
    assert.match(output, /8 followed · 1 need attention/);
    assert.match(output, /> Goal 7/);
    assert.match(output, /2 more followed/);
    assert.match(output, /KiB brief text/);
    for (const width of [0, 1, 2, 12, 45, 80]) assert.ok(widget.render(width).every((line) => visibleWidth(line) <= width));
  }
});

test("singleton brief warnings remain visible after another goal is removed", () => {
  const value = snapshot();
  const healthy = { focus: { id: "t-999999", project: value.project }, snapshot: snapshot("t-999999") };
  for (const warning of ["Brief unavailable: removed", "Brief truncated; read the reference"]) {
    const view = { focus: { id: "t-123abc", project: value.project }, snapshot: value, brief: { text: "", warning } };
    const widget = new FollowedWidget(), theme = getThemeByName("dark");
    widget.update([view, healthy], healthy.focus.id, theme);
    assert.match(plain(widget.render(160)), /1 need attention/);
    widget.update([view], view.focus.id, theme);
    assert.match(plain(widget.render(160)), /brief needs attention/);
    assert.ok(overview(view).includes(warning));
  }
});

test("orientation limits task detail, retains the brief and reports omissions and unknowns", () => {
  const value = snapshot();
  value.overview.tasks = Array.from({ length: 120 }, (_, n) => task(`t-${n.toString(16)}`, `slice ${n}`));
  const model = { focus: { id: "t-123abc", project: value.project }, snapshot: value, brief: { text: "accepted pattern", warning: "Brief truncated; read the reference" } };
  const content = orientation(model);
  assert.match(content, /80 tasks omitted/);
  assert.match(content, /accepted pattern/);
  assert.match(content, /Brief truncated/);
  assert.match(content, /not verified landed/);
  assert.doesNotMatch(content, /slice 40/);
  const history = inspection({ ...model, history: true });
  assert.match(history, /20 tasks omitted; use dots task goal show t-123abc --all --json/);
  assert.match(inspection(model), /20 tasks omitted; use dots task goal show t-123abc --json/);
  const unavailable = inspection({ focus: model.focus, error: "CLI unavailable" });
  assert.match(unavailable, /CLI unavailable/);
  assert.doesNotMatch(unavailable, /0 done/);
});
