import assert from 'node:assert/strict';
import test from 'node:test';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {goalFixtures} from '../dev/goals.ts';
import {GoalPanel} from '../extensions/goal/panel.ts';
import type {GoalView} from '../extensions/goal/present.ts';
import {orientation} from '../extensions/goal/context.ts';
import {
  goalTree,
  groupSummary,
  treeRows,
} from '../extensions/goal/hierarchy.ts';
import {
  withHierarchy,
  type GoalSnapshot,
  type GoalTask,
} from '../extensions/goal/source.ts';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';

initTheme('dark', false);
const theme = getThemeByName('dark')!;
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
const panel = (view: GoalView, height = 30) => {
  const result = new GoalPanel(
    {matches: () => false},
    {release() {}, close() {}, cycle() {}},
    () => {},
    () => height,
  );
  result.focused = true;
  result.setGoal({view, followed: 1}, theme);
  return result;
};

test('group totals use complete leaf records, not visible children or group statuses', () => {
  const model = goalFixtures(8)[0]!;
  const snapshot = model.snapshot!;
  const original = structuredClone(model);
  const tree = goalTree(snapshot);
  assert.equal(tree.warning, undefined);
  assert.equal(tree.nodes.get('t-888888')?.totals?.total, 3);
  assert.equal(
    groupSummary(tree.nodes.get('t-888888')!),
    '1/3 done · 1 claimed · 1 waiting',
  );
  assert.equal(
    groupSummary(tree.nodes.get('t-999999')!),
    '0/1 done · 1 waiting',
  );
  assert.equal(groupSummary(tree.nodes.get('t-aaaaaa')!), '1/3 done · 1 held');
  assert.deepEqual(
    treeRows(tree).map(row => [row.node.task.id, row.depth]),
    [
      ['t-888888', 0],
      ['t-333333', 1],
      ['t-999999', 1],
      ['t-444444', 2],
      ['t-aaaaaa', 0],
      ['t-cccccc', 1],
      ['t-555555', 1],
    ],
  );
  assert.doesNotMatch(
    orientation(model),
    /Define shared types|Document shared types/,
  );
  assert.match(orientation(model), /Tasks: 3 open · 1 held · 2 done/);
  assert.deepEqual(model, original);
});

test('dropped leaves remain in denominators, released tasks are not claims, and closed groups retain ancestry', () => {
  const base = goalFixtures(8)[0]!.snapshot!;
  const allTasks = base.allTasks!.map((task): GoalTask =>
    task.id === 't-333333'
      ? {...task, owner: null}
      : task.id === 't-bbbbbb'
        ? {...task, status: 'dropped'}
        : task.id === 't-888888'
          ? {...task, status: 'done'}
          : task,
  );
  const snapshot = withHierarchy({
    ...base,
    allTasks,
    overview: {
      ...base.overview,
      progress: {...base.overview.progress, done: 1, dropped: 1, claimed: 0},
    },
  });
  const tree = goalTree(snapshot);
  assert.equal(tree.warning, undefined);
  assert.equal(
    groupSummary(tree.nodes.get('t-888888')!),
    '1/3 done · 1 waiting',
  );
  assert.equal(
    groupSummary(tree.nodes.get('t-aaaaaa')!),
    '0/3 done · 1 held · 1 dropped',
  );
  assert.equal(
    snapshot.overview.tasks.some(task => task.id === 't-888888'),
    false,
  );
  assert.equal(
    treeRows(tree)[0]?.node.task.id,
    't-888888',
    'closed ancestors remain as structural summaries',
  );
  assert.equal(treeRows(tree)[1]?.depth, 1);
});

test('folding is presentation-only, nested, preserved across refresh, and history reveals closed leaves', () => {
  const model = goalFixtures(8)[0]!;
  const inspector = panel(model);
  const lines = inspector.render(160).map(stripTerminalSequences);
  assert.equal(
    lines.filter(line => line.startsWith('╭')).length,
    1,
    'one Inspector frame around unframed sections',
  );
  assert.match(
    lines.join('\n'),
    /Now[\s\S]*Tasks[\s\S]*Result[\s\S]*Recorded state · not live execution/,
  );
  assert.match(plain(inspector.render(160)), /Consumer migration/);
  inspector.handleInput('\x1b[D'); // Fold selected Core interface.
  let output = plain(inspector.render(160));
  assert.doesNotMatch(
    output,
    /Consumer migration|Update exports|Adapt consumers/,
  );
  assert.match(output, /Core interface · 1\/3 done · 1 claimed/);
  assert.match(output, /Documentation/);
  inspector.setGoal({view: model, followed: 1}, theme);
  assert.doesNotMatch(plain(inspector.render(160)), /Consumer migration/);
  inspector.handleInput('\x1b[C');
  assert.match(plain(inspector.render(160)), /Consumer migration/);
  inspector.handleInput(']'); // Select nested Consumer migration.
  inspector.handleInput(' ');
  output = plain(inspector.render(160));
  assert.match(output, /Consumer migration/);
  assert.doesNotMatch(output, /Adapt consumers/);
  assert.match(output, /Update exports/);
  inspector.handleInput('2');
  assert.match(plain(inspector.render(160)), /Two workstreams/);
  inspector.handleInput('1');
  assert.doesNotMatch(plain(inspector.render(160)), /Adapt consumers/);
  inspector.handleInput(' ');
  assert.match(plain(inspector.render(160)), /Adapt consumers/);
  const history = goalFixtures(9)[0]!;
  inspector.setGoal({view: history, followed: 1}, theme);
  assert.match(plain(inspector.render(160)), /Define shared types/);
  assert.match(plain(inspector.render(160)), /Document shared types/);
  assert.equal(treeRows(goalTree(history.snapshot!)).length, 9);
});

test('partial, cyclic and duplicate hierarchies never report group completion fractions', () => {
  const partial = goalFixtures(10)[0]!;
  const base = goalFixtures(8)[0]!.snapshot!;
  const originals = base.allTasks!;
  const cyclic = originals.map(task =>
    task.id === 't-888888' ? {...task, parent: 't-999999'} : task,
  );
  for (const snapshot of [
    partial.snapshot!,
    {...base, allTasks: cyclic},
    {...base, allTasks: [...originals, originals[0]!]},
  ]) {
    const tree = goalTree(snapshot);
    assert.match(tree.warning!, /Hierarchy incomplete/);
    for (const node of tree.nodes.values())
      assert.equal(node.totals, undefined);
    assert.ok(treeRows(tree).length <= originals.length + 1);
    const output = plain(panel({...partial, snapshot}, 60).render(160));
    assert.match(output, /totals unavailable/);
    assert.doesNotMatch(output, /1\/3 done|0\/1 done/);
  }
});

test('nested rows remain bounded in both themes; deep selection scrolls into view', () => {
  for (const name of ['light', 'dark'])
    for (const phase of [8, 9, 10]) {
      const view = goalFixtures(phase)[0]!;
      const theme = getThemeByName(name)!;
      const component = panel(view, 40);
      for (const width of [0, 1, 2, 12, 30, 60, 120, 240]) {
        const lines = component.render(width);
        assert.ok(lines.length < 45);
        assert.ok(
          lines.every(line => visibleWidth(line) <= width),
          `${name}/${phase}/${width}: ${plain(lines)}`,
        );
        assert.doesNotMatch(plain(lines), /\uFFFD|\x1b|\x07/);
      }
    }
  const model = goalFixtures(8)[0]!;
  const base = model.snapshot!;
  const prototype = base.allTasks![0]!;
  const tasks: GoalTask[] = Array.from({length: 80}, (_, i) => ({
    ...prototype,
    id: `t-${i}`,
    title: `Nested group ${i}`,
    status: 'open',
    children: 1,
    parent: i ? `t-${i - 1}` : base.overview.goal.root.id,
  }));
  tasks.push({
    ...prototype,
    id: 't-end',
    parent: 't-79',
    children: 0,
    status: 'open',
    title: 'Deep leaf',
  });
  const snapshot: GoalSnapshot = {
    ...base,
    allTasks: tasks,
    overview: {
      ...base.overview,
      goal: {
        ...base.overview.goal,
        root: {...base.overview.goal.root, children: 1},
      },
      tasks,
      hidden_closed: 0,
      progress: {
        total: 1,
        done: 0,
        held: 0,
        open: 1,
        dropped: 0,
        claimed: 0,
        ready: 1,
        review: 0,
      },
    },
  };
  assert.equal(goalTree(snapshot).warning, undefined);
  const inspector = panel({...model, snapshot}, 6);
  inspector.render(120);
  for (let i = 0; i < 15; i++) {
    inspector.handleInput(']');
    inspector.render(120);
  }
  assert.match(plain(inspector.render(120)), /Nested group 15/);
  assert.match(plain(inspector.render(120)), /level 16/);
});
