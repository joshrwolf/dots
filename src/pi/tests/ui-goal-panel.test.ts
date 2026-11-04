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
  panelWidth,
  summaryRow,
} from '../extensions/goal/panel.ts';
import type {GoalView} from '../extensions/goal/present.ts';
import type {GoalTask} from '../extensions/goal/source.ts';
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

test('the popup is large and centered and fills small terminals', () => {
  assert.equal(panelOverlay.anchor, 'center');
  assert.equal(PANEL_KEY, 'alt+g');
  assert.equal(panelHeight(60), 49);
  assert.equal(panelHeight(24), 22, 'short screens use every row');
  assert.equal(panelHeight(2), 1);
  assert.equal(panelWidth(200), 180);
  assert.equal(panelWidth(100), 90);
  assert.equal(panelWidth(60), 60, 'narrow screens use every column');
});

test('the panel shows live work, the claimed task tree and the goal result without changing records', () => {
  for (let phase = 0; phase < 14; phase++) {
    const views = goalFixtures(phase);
    const original = structuredClone(views);
    const context = views.map(view => orientation(view));
    for (const name of ['dark', 'light']) {
      const view = new GoalPanel(
        {matches: () => false},
        {close() {}, cycle() {}},
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
        assert.equal(
          lines.length,
          width ? 42 : 0,
          'the overview fills the popup',
        );
        assert.ok(
          lines.every(line => visibleWidth(line) <= width),
          `${phase}/${name}/${width}`,
        );
        assert.doesNotMatch(plain(lines), /\uFFFD|\x1b|\x07/);
      }
      const output = plain(view.render(120));
      if (phase === 0) {
        assert.match(output, /→ ● Claimed +Update exports · you/);
        assert.match(output, /Detail[\s\S]*Claimed by this session/);
      }
      if (phase === 1) assert.match(output, /! Held +Resolve compatibility/);
      if (phase === 4) assert.match(output, /Unavailable/);
      if (phase === 5) assert.match(output, /Loading goal/);
      if (phase === 6) {
        const header = plain(view.render(80)).split('\n')[1]!;
        assert.match(header, /slices done · .*brief truncated · 2 brief +│/);
        assert.doesNotMatch(plain(view.render(80)), /from line 201/);
        view.handleInput('2');
        assert.match(plain(view.render(80)), /Brief truncated; read/);
        view.handleInput('1');
      }
      if (phase === 7) assert.match(output, /No slices recorded/);
      if (phase === 8) assert.match(output, /Shared types landed/);
      if (phase === 11) {
        assert.match(output, /1 worker/);
        assert.match(output, /Worker exports-worker · edit src\/exports\.ts/);
      }
      if (phase === 12) assert.match(output, /Worker exports-worker · asking/);
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
  assert.match(plain(view.render(120)), /Tab next goal · Esc\/alt\+g close/);
  view.handleInput('2');
  assert.match(plain(view.render(120)), /Two workstreams/);
  view.setGoal({view: goal, followed: 2}, getThemeByName('light')!);
  assert.match(
    plain(view.render(120)),
    /Two workstreams/,
    'refresh keeps section',
  );
  view.setGoal({view: goalFixtures(3)[1]!, followed: 2}, theme);
  assert.match(
    plain(view.render(120)),
    /Tasks[\s\S]*Detail/,
    'new goal resets',
  );
  view.handleInput('3');
  assert.match(plain(view.render(120)), /Focus only; no automatic execution/);
  view.handleInput('\t');
  view.handleInput('\x1b');
  view.handleInput('\x1bg');
  assert.deepEqual(calls, ['cycle', 'close', 'close']);
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
  assert.doesNotMatch(plain(hidden.render(120)), /Define shared types/);
  assert.match(plain(hidden.render(120)), /2 closed hidden · h shows them/);
  hidden.handleInput('h');
  assert.match(plain(hidden.render(120)), /Define shared types/);
  assert.match(plain(hidden.render(120)), /with closed/);
});

test('the graph lists leaves by dependency; the outline folds groups', () => {
  const goal = goalFixtures(8)[0]!;
  const overview = goal.snapshot!.overview;
  const noted = panel({
    ...goal,
    snapshot: {
      ...goal.snapshot!,
      overview: {
        ...overview,
        open_graph: {
          ...overview.open_graph!,
          notes: ['cycle: t-444444 waits on t-555555; edge omitted'],
        },
      },
    },
  }).result;
  assert.match(
    plain(noted.render(160)),
    /Document the interface[\s\S]*cycle: t-444444 waits on t-555555; edge omitted/,
    'what the graph omits is listed with it',
  );
  noted.handleInput('o');
  assert.doesNotMatch(plain(noted.render(160)), /cycle:/);
  const {result: view} = panel(goal);
  const detail = () =>
    plain(view.render(160))
      .split('\n')
      .map(line => line.split(' │ ')[1] ?? '')
      .join('\n');
  assert.match(
    detail(),
    /Update exports[\s\S]*Claimed by this session/,
    'starts on this session’s claim',
  );
  assert.match(
    plain(view.render(160)),
    /→ ● Claimed +Update exports · Core interface · you.*\n│ +│ .*\n.*◌ Waiting +Adapt consumers · Consumer migration/,
    'dots draws the edge; the group follows the title',
  );
  view.handleInput('\x1b[B');
  assert.match(detail(), /Adapt consumers[\s\S]*Needs: Update exports · open/);
  view.handleInput('h');
  assert.match(
    plain(view.render(160)),
    /✓ +Done +Define shared types[\s\S]*├─╮[\s\S]*→ ◌ │ Waiting +Adapt consumers/,
    'closed leaves join the graph and keep the selection',
  );
  view.handleInput('h');
  view.handleInput('o');
  assert.match(
    plain(view.render(160)),
    /→ ◌ Waiting +Adapt consumers[\s\S]*o graph/,
  );
  view.handleInput('\x1b[A');
  assert.match(plain(view.render(160)), /→ ▾ Consumer migration/);
  assert.match(detail(), /0\/1 done/);
  view.handleInput('\x1b[D');
  assert.doesNotMatch(
    plain(view.render(160)),
    /Adapt consumers/,
    'left folds a group',
  );
  view.handleInput('\x1b[C');
  view.handleInput('\x1b[B');
  assert.match(detail(), /Adapt consumers[\s\S]*Needs: Update exports · open/);
  view.handleInput('\x1b[D');
  assert.match(
    plain(view.render(160)),
    /→ ▾ Consumer migration/,
    'left on a leaf selects its parent',
  );
  const narrow = plain(view.render(80));
  assert.match(
    narrow,
    /Tasks[\s\S]*Detail/,
    'narrow panes stack detail under the tree',
  );
  assert.doesNotMatch(narrow, / │ /);
});

test('a stacked popup gives the list most rows and bounds the detail', () => {
  const leaf = (n: number, status: GoalTask['status']): GoalTask => ({
    ...goalFixtures(8)[0]!.snapshot!.overview.tasks[0]!,
    id: `t-${n}`,
    title: `Slice ${n}`,
    parent: 't-111111',
    status,
    owner: null,
    result:
      status === 'done' ? 'Verified.\n\nLong result\nwith\nmany\nlines.' : null,
  });
  const tasks = Array.from({length: 15}, (_, n) =>
    leaf(n, n < 11 ? 'done' : 'open'),
  );
  const goal = goalFixtures(0)[0]!;
  const graph = {
    rows: tasks.map(task => ({
      task: task.id,
      lines: [task.status === 'done' ? '✓' : '○', '│'],
    })),
    notes: [],
  };
  const view: GoalView = {
    ...goal,
    snapshot: {
      ...goal.snapshot!,
      allTasks: tasks,
      overview: {
        ...goal.snapshot!.overview,
        tasks,
        graph,
        open_graph: {...graph, rows: graph.rows.slice(11)},
      },
    },
  };
  const panel = new GoalPanel(
    {matches: () => false},
    {close() {}, cycle() {}},
    () => {},
    () => panelHeight(32),
  );
  panel.setGoal({view, followed: 1}, theme);
  panel.handleInput('h');
  const lines = plain(panel.render(80)).split('\n');
  const tasksAt = lines.findIndex(line => /Tasks ─/.test(line));
  const detailAt = lines.findIndex(line => /Detail ─/.test(line));
  const body = lines.length - 2 - tasksAt;
  assert.ok(detailAt - tasksAt >= (body * 2) / 3 - 1, lines.join('\n'));
  assert.ok(
    lines.length - 1 - detailAt <= Math.max(4, body / 3),
    lines.join('\n'),
  );
  assert.match(lines.join('\n'), /… \d+ more lines · 3 record/);
});

test('short popups give rows to the tree and detail before the goal result', () => {
  const goal = goalFixtures(8)[0]!;
  const tasks = (output: string) =>
    output
      .split('\n')
      .filter(line =>
        /(Done|Claimed|Waiting|Held|Open) {2,}\w/.test(line.split(' │ ')[0]!),
      ).length;
  const at = (width: number, height: number) => {
    const view = new GoalPanel(
      {matches: () => false},
      {close() {}, cycle() {}},
      () => {},
      () => height,
    );
    view.setGoal({view: goal, followed: 1, owner: 'fixture-reviewer'}, theme);
    const output = plain(view.render(width));
    assert.equal(view.render(width).length, height + 2);
    assert.doesNotMatch(output, / 0 of /);
    return output;
  };
  let output = at(160, 10);
  assert.equal(tasks(output), 4, output);
  assert.match(output, /Goal result ─/, 'a heading and one line fit');
  output = at(160, 9);
  assert.equal(tasks(output), 4, output);
  assert.match(output, /Goal result Shared types landed/, 'one line');
  output = at(160, 8);
  assert.equal(tasks(output), 4, output);
  assert.doesNotMatch(output, /Goal result/);
  assert.match(output, /goal result: 3 record/);
  assert.match(output, /Claimed by this session/, 'detail keeps its rows');
  output = at(90, 12);
  assert.equal(tasks(output), 4, output);
  assert.match(output, /Detail[\s\S]*Update exports/, 'stacked detail stays');
});
