import assert from 'node:assert/strict';
import test from 'node:test';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {PriorityRow} from '@joshrwolf/pi-kit/ui';
import {goalActivityFixture, goalFixtures} from '../dev/goals.ts';
import {
  GoalPanel,
  PANEL_KEY,
  panelHeight,
  panelOverlay,
  summaryRow,
} from '../extensions/goal/panel.ts';
import type {GoalView} from '../extensions/goal/present.ts';
import {orientation} from '../extensions/goal/context.ts';
import {
  workerFrame,
  projectWorkerActivity,
  type WorkerActivitySnapshot,
} from '../src/ui/worker-activity.ts';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';

initTheme('dark', false);
const theme = getThemeByName('dark')!;
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
const activity = (
  project: string,
  id: string,
  state: 'running' | 'asking' | 'quiet' = 'running',
): WorkerActivitySnapshot => ({
  frame: 2,
  workers: [{assignment: {project, id}, label: 'fixture-worker', state}],
});
const summary = (
  views: readonly GoalView[],
  live?: WorkerActivitySnapshot,
  width = 200,
  owner?: string,
  selected = views[0]?.focus.id,
) => {
  const row = new PriorityRow();
  row.update(summaryRow(views, selected, live, owner), theme);
  return row.render(width);
};
const cue = (view: GoalView, live?: WorkerActivitySnapshot) =>
  plain(summary([view], live)).match(/^◎ ?(\S)? /)?.[1] ?? '';
const withProgress = (view: GoalView, review: number): GoalView => ({
  ...view,
  snapshot: {
    ...view.snapshot!,
    overview: {
      ...view.snapshot!.overview,
      progress: {...view.snapshot!.overview.progress, review},
    },
  },
});
function panel(view: GoalView, followed = 1) {
  const calls: string[] = [];
  const result = new GoalPanel(
    {
      matches: (data, action) =>
        data === '\x1b' && action === 'tui.select.cancel',
    },
    {
      release: () => calls.push('release'),
      close: () => calls.push('close'),
      cycle: () => calls.push('cycle'),
    },
    () => {},
    () => 40,
  );
  result.setGoal({view, followed, owner: 'fixture-reviewer'}, theme);
  return {result, calls};
}

test('the summary cue animates only for a live assignment in the exact project and goal tree', () => {
  const view = goalFixtures(11)[0]!;
  assert.equal(cue(view), '·', 'claims are not execution');
  assert.equal(cue(view, activity('/workspace/other', 't-333333')), '·');
  assert.equal(cue(view, activity(view.focus.project, 't-absent')), '·');
  for (const id of ['t-111111', 't-888888', 't-333333'])
    assert.equal(cue(view, activity(view.focus.project, id)), workerFrame(2));
  assert.equal(
    cue(view, activity(view.focus.project, 't-333333', 'asking')),
    '?',
  );
  assert.equal(
    cue(view, activity(view.focus.project, 't-333333', 'quiet')),
    '·',
  );
  assert.equal(
    cue({...view, error: 'offline'}, activity(view.focus.project, 't-333333')),
    '!',
  );
  assert.equal(cue({focus: view.focus}), '·');
  const review = withProgress(view, 1);
  assert.equal(cue(review, activity(view.focus.project, 't-333333')), '');
  assert.match(plain(summary([review])), /^◎ Unify[^◇]*1 review/);
  assert.equal(
    cue(goalFixtures(8)[0]!, activity(view.focus.project, 't-333333')),
    '!',
    'held work outranks animation',
  );
  assert.equal(cue(goalFixtures(2)[0]!), '✓');
  assert.equal(cue(goalFixtures(10)[0]!), '!', 'partial tree');
});

test('the persistent surface is one bounded line naming progress, current work and other follows', () => {
  for (const phase of [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]) {
    const views = goalFixtures(phase);
    const original = structuredClone(views);
    for (const name of ['dark', 'light'])
      for (const width of [0, 1, 2, 12, 30, 80, 160, 400]) {
        const row = new PriorityRow();
        row.update(
          summaryRow(
            views,
            views[0]!.focus.id,
            goalActivityFixture(phase, 2),
            'fixture-reviewer',
          ),
          getThemeByName(name)!,
        );
        const lines = row.render(width);
        assert.ok(lines.length <= 1, `${phase}/${width}`);
        assert.ok(lines.every(line => visibleWidth(line) <= width));
        assert.doesNotMatch(plain(lines), /\uFFFD|\x07/);
      }
    assert.deepEqual(views, original);
  }
  const nested = goalFixtures(11);
  assert.match(
    plain(summary(nested, goalActivityFixture(11, 2), 200, 'fixture-reviewer')),
    /^◎ \S Unify the export interface · 2\/6 done · now: Update exports · 1 worker .*alt\+g$/,
  );
  assert.match(
    plain(summary(goalFixtures(12), goalActivityFixture(12, 2))),
    /1 asking/,
  );
  assert.match(plain(summary(goalFixtures(4))), /unavailable · \/goal refresh/);
  assert.match(plain(summary(goalFixtures(5))), /loading/);
  assert.match(plain(summary(goalFixtures(6))), /brief warning/);
  assert.match(plain(summary(goalFixtures(2))), /4\/4 done · done/);
  const many = goalFixtures(3);
  assert.match(plain(summary(many)), /\+2 followed/);
  const live = activity(many[2]!.focus.project, many[2]!.focus.id, 'asking');
  assert.match(plain(summary(many, live)), /\+2 followed · needs attention/);
  assert.doesNotMatch(plain(summary(many, live)), /Align CLI output/);
  const long = goalFixtures(11)[0]!;
  const unicode = {
    ...long,
    snapshot: {
      ...long.snapshot!,
      overview: {
        ...long.snapshot!.overview,
        goal: {
          ...long.snapshot!.overview.goal,
          root: {
            ...long.snapshot!.overview.goal.root,
            title: '日本語 👩‍💻 '.repeat(80),
          },
        },
      },
    },
  };
  assert.ok(
    summary([unicode], undefined, 30).every(l => visibleWidth(l) <= 30),
  );
});

test('worker activity projection carries labels and current work, not terminal history', () => {
  const assigned = {project: '/workspace/example', id: 't-333333'};
  const snapshot = projectWorkerActivity(
    [
      {
        assignment: assigned,
        label: 'a',
        state: 'running',
        idleMs: 0,
        detail: 'read x',
        headline: 'Checking',
      },
      {assignment: assigned, label: 'b', state: 'running', idleMs: 90_000},
      {assignment: assigned, label: 'c', state: 'asking', idleMs: 90_000},
      {
        assignment: assigned,
        label: 'd',
        state: 'running',
        idleMs: 0,
        failed: 'call failed',
        phase: 'retrying',
      },
      {assignment: assigned, label: 'e', state: 'completed', idleMs: 0},
      {
        assignment: assigned,
        label: 'f',
        state: 'running',
        idleMs: 0,
        finished: true,
      },
      {label: 'g', state: 'running', idleMs: 0},
    ],
    4,
    90_000,
  );
  assert.equal(snapshot.frame, 4);
  assert.deepEqual(
    snapshot.workers.map(worker => [worker.label, worker.state, worker.doing]),
    [
      ['a', 'running', 'Checking'],
      ['b', 'quiet', undefined],
      ['c', 'asking', undefined],
      ['d', 'quiet', 'retrying'],
    ],
  );
});

test('the panel overlay keeps editor input and fits narrow terminals', () => {
  assert.equal(panelOverlay.nonCapturing, true);
  assert.equal(panelOverlay.anchor, 'top-right');
  assert.equal(PANEL_KEY, 'alt+g');
  assert.equal(panelHeight(60), 40);
  assert.equal(panelHeight(2), 1);
});

test('the panel shows live work, the claimed task tree and the goal result without changing records', () => {
  for (let phase = 0; phase < 14; phase++) {
    const views = goalFixtures(phase);
    const original = structuredClone(views);
    const context = views.map(view => orientation(view));
    for (const name of ['dark', 'light']) {
      const view = new GoalPanel(
        {matches: () => false},
        {release() {}, close() {}, cycle() {}},
        () => {},
        () => 40,
      );
      view.setGoal(
        {
          view: views[0]!,
          activity: goalActivityFixture(phase, 2),
          owner: 'fixture-reviewer',
          followed: views.length,
        },
        getThemeByName(name)!,
      );
      for (const width of [0, 1, 2, 12, 30, 60, 120, 240]) {
        const lines = view.render(width);
        assert.ok(lines.length <= 42);
        assert.ok(
          lines.every(line => visibleWidth(line) <= width),
          `${phase}/${name}/${width}`,
        );
        assert.doesNotMatch(plain(lines), /\uFFFD|\x1b|\x07/);
      }
      const output = plain(view.render(120));
      if (phase === 0) {
        assert.match(output, /› you · Update exports/);
        assert.match(output, /Needs: Update exports/);
      }
      if (phase === 1) assert.match(output, /! Held +Resolve compatibility/);
      if (phase === 4) assert.match(output, /Unavailable/);
      if (phase === 5) assert.match(output, /Loading goal/);
      if (phase === 6) assert.match(output, /Brief truncated/);
      if (phase === 7) assert.match(output, /No slices recorded/);
      if (phase === 8) assert.match(output, /Shared types landed/);
      if (phase === 11)
        assert.match(
          output,
          /exports-worker · Update exports · edit src\/exports\.ts/,
        );
      if (phase === 12) assert.match(output, /exports-worker · .* · asking/);
    }
    assert.deepEqual(views, original);
    assert.deepEqual(
      views.map(view => orientation(view)),
      context,
    );
  }
});

test('panel keys select sections, closed tasks and goals; Esc returns input and the shortcut closes', () => {
  const goal = goalFixtures(9)[0]!;
  const {result: view, calls} = panel(goal, 2);
  assert.match(
    plain(view.render(120)),
    /\/goal to scroll and fold · alt\+g close/,
  );
  view.focused = true;
  assert.match(plain(view.render(120)), /Tab next goal/);
  view.handleInput('2');
  assert.match(plain(view.render(120)), /Two workstreams/);
  view.setGoal({view: goal, followed: 2}, getThemeByName('light')!);
  assert.match(
    plain(view.render(120)),
    /Two workstreams/,
    'refresh keeps section',
  );
  view.setGoal({view: goalFixtures(3)[1]!, followed: 2}, theme);
  assert.match(plain(view.render(120)), /Now[\s\S]*Tasks/, 'new goal resets');
  view.handleInput('3');
  assert.match(plain(view.render(120)), /Focus only; no automatic execution/);
  view.handleInput('\t');
  view.handleInput('\x1b');
  view.handleInput('\x1bg');
  assert.deepEqual(calls, ['cycle', 'release', 'close']);
  const nested = goalFixtures(8)[0]!;
  const hidden = panel({
    ...nested,
    snapshot: {
      ...nested.snapshot!,
      overview: {
        ...nested.snapshot!.overview,
        tasks: nested.snapshot!.overview.tasks.filter(
          task => task.status !== 'done',
        ),
        hidden_closed: 2,
      },
    },
  }).result;
  hidden.focused = true;
  assert.doesNotMatch(plain(hidden.render(120)), /Define shared types/);
  assert.match(
    plain(hidden.render(120)),
    /2 closed tasks hidden · h shows them/,
  );
  hidden.handleInput('h');
  assert.match(plain(hidden.render(120)), /Define shared types/);
  assert.match(plain(hidden.render(120)), /with closed/);
});
