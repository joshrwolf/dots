import assert from 'node:assert/strict';
import test from 'node:test';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {Panel, PriorityRow, readableForeground} from '@joshrwolf/pi-dots/ui';
import * as kit from '@joshrwolf/pi-kit/ui';
import {
  FollowedWidget,
  GoalWidget,
  goalOverview,
} from '../extensions/goal/dashboard.ts';
import {GoalInspector} from '../extensions/goal/present.ts';
import {orientation} from '../extensions/goal/context.ts';
import {goalFixtures} from '../dev/goals.ts';
// Pinned test harness only; production uses the supplied host theme.
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';

initTheme('dark', false);
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));

test('companion re-exports the shared primitive identities', () => {
  assert.equal(Panel, kit.Panel);
  assert.equal(PriorityRow, kit.PriorityRow);
  assert.equal(readableForeground, kit.readableForeground);
});

test('dashboards retain recorded facts and original context across states, themes and widths', () => {
  for (let phase = 0; phase < 8; phase++) {
    const views = goalFixtures(phase);
    const original = structuredClone(views);
    const context = views.map(view => orientation(view));
    for (const name of ['dark', 'light']) {
      const theme = getThemeByName(name)!;
      const widget = new FollowedWidget();
      widget.update(views, views[0]!.focus.id, theme);
      for (const component of [widget, goalOverview(views[0]!, theme)]) {
        for (const width of [0, 1, 2, 12, 30, 60, 120, 240, 400]) {
          const lines = component.render(width);
          assert.ok(lines.length < 30);
          assert.ok(
            lines.every(line => visibleWidth(line) <= width),
            `${phase}/${name}/${width}`,
          );
          assert.doesNotMatch(plain(lines), /\uFFFD|\x1b|\x07/);
        }
        const result = plain(component.render(120));
        assert.doesNotMatch(result, /running|landed|Current:/i);
        if (component === widget) {
          assert.ok(component.render(120).length <= (phase === 3 ? 6 : 4));
          if (phase === 0) assert.match(result, /○ Adapt consumers/);
          if (phase === 1) assert.match(result, /! Resolve compatibility/);
          if (phase === 2) assert.match(result, /4\/4/);
          if (phase === 4) assert.match(result, /unavailable · refresh/);
          if (phase === 5) assert.match(result, /loading/);
          if (phase === 6) assert.match(result, /brief warning/);
          if (phase === 7) assert.doesNotMatch(result, /0\/0|█/);
        } else {
          if (phase === 0) assert.match(result, /✓ Done +Define shared types/);
          if (phase === 1)
            assert.match(result, /! Held +Resolve compatibility/);
          if (phase === 2) assert.match(result, /4\/4 slices done/);
          if (phase === 4) assert.match(result, /Unavailable/);
          if (phase === 5) assert.match(result, /Loading goal/);
          if (phase === 6) assert.match(result, /Brief truncated/);
          if (phase === 7) assert.match(result, /No slices recorded/);
        }
      }
    }
    assert.deepEqual(views, original);
    assert.deepEqual(
      views.map(view => orientation(view)),
      context,
    );
  }
});

test('short previews favor claims and blockers, report omissions, and never elevate expired claims', () => {
  const first = goalFixtures()[0]!;
  const base = first.snapshot!;
  const prototype = base.overview.tasks[0]!;
  const tasks = Array.from({length: 9}, (_, i) => ({
    ...prototype,
    id: `t-${i}`,
    title: `Recorded ${i}`,
  }));
  tasks.push({
    ...prototype,
    id: 't-999999',
    title: 'Needs attention',
    status: 'held',
  });
  tasks.push({
    ...prototype,
    id: 't-aaaaaa',
    title: 'Current claim',
    status: 'open',
    owner: 'fixture',
  });
  tasks.push({
    ...prototype,
    id: 't-bbbbbb',
    title: 'Released claim',
    status: 'open',
    owner: null,
  });
  const model = {
    ...first,
    snapshot: {
      ...base,
      overview: {
        ...base.overview,
        tasks,
        progress: {
          total: 12,
          done: 9,
          open: 2,
          held: 1,
          claimed: 1,
          dropped: 0,
          ready: 1,
          review: 0,
        },
      },
    },
  };
  const widget = new GoalWidget();
  widget.update(model, getThemeByName('dark')!);
  const output = plain(widget.render(120));
  assert.match(output, /! Needs attention/);
  assert.match(output, /more visible slices/);
  assert.match(output, /› Current claim/);
  assert.doesNotMatch(output, /Expired claim|running|Claimed/);
  assert.equal(widget.render(120).length, 4);
});

test('inspection keeps the selected section through refresh and resets it for a different goal', () => {
  const goal = goalFixtures()[0]!;
  const theme = getThemeByName('dark')!;
  const inspector = new GoalInspector(
    {matches: () => false},
    () => {},
    () => {},
    () => 20,
  );
  inspector.setGoal(goal, theme, () => {});
  inspector.handleInput('2');
  assert.match(plain(inspector.render(120)), /Preserve compatibility/);
  inspector.setGoal(goal, getThemeByName('light')!, () => {});
  assert.match(plain(inspector.render(120)), /Preserve compatibility/);
  inspector.setGoal(goalFixtures(3)[1]!, theme, () => {});
  assert.doesNotMatch(plain(inspector.render(120)), /Preserve compatibility/);
  assert.match(plain(inspector.render(120)), /Slices/);
});
