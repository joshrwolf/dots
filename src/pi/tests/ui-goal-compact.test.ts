import assert from 'node:assert/strict';
import test from 'node:test';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {goalActivityFixture, goalFixtures} from '../dev/goals.ts';
import {compactGoals, goalCue} from '../extensions/goal/compact.ts';
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
  workers: [{assignment: {project, id}, state}],
});

test('a selected goal animates only for a matching live assignment in its exact project and ancestry', () => {
  const view = goalFixtures(11)[0]!;
  assert.equal(goalCue(view), '·', 'claims are not execution');
  assert.equal(goalCue(view, activity('/workspace/other', 't-333333')), '·');
  assert.equal(goalCue(view, activity(view.focus.project, 't-absent')), '·');
  for (const id of ['t-111111', 't-888888', 't-333333'])
    assert.equal(
      goalCue(view, activity(view.focus.project, id)),
      workerFrame(2),
    );
  assert.equal(
    goalCue(view, activity(view.focus.project, 't-333333', 'asking')),
    '?',
  );
  assert.equal(
    goalCue(view, activity(view.focus.project, 't-333333', 'quiet')),
    '·',
  );
  assert.equal(
    goalCue(view, {
      frame: 3,
      workers: [
        ...activity(view.focus.project, 't-333333').workers,
        ...activity(view.focus.project, 't-888888', 'asking').workers,
      ],
    }),
    '?',
  );
  assert.equal(
    goalCue(
      {...view, error: 'offline'},
      activity(view.focus.project, 't-333333'),
    ),
    '!',
  );
  assert.equal(
    goalCue({focus: view.focus}, activity(view.focus.project, 't-333333')),
    '·',
  );
  assert.equal(
    goalCue(
      {
        ...view,
        snapshot: {
          ...view.snapshot!,
          overview: {
            ...view.snapshot!.overview,
            progress: {...view.snapshot!.overview.progress, review: 1},
          },
        },
      },
      activity(view.focus.project, 't-333333'),
    ),
    '◇',
    'review outranks animation',
  );
  assert.equal(
    goalCue(goalFixtures(8)[0]!, activity(view.focus.project, 't-333333')),
    '!',
    'held work outranks animation',
  );
  assert.equal(
    goalCue(goalFixtures(2)[0]!, activity(view.focus.project, 't-333333')),
    '✓',
  );
  assert.equal(
    goalCue(goalFixtures(10)[0]!, activity(view.focus.project, 't-333333')),
    '!',
    'partial tree outranks animation',
  );
});

test('review diamonds stay on slices, not beside the goal identity', () => {
  const view = goalFixtures(0)[0]!;
  const review = {
    ...view,
    snapshot: {
      ...view.snapshot!,
      overview: {
        ...view.snapshot!.overview,
        progress: {...view.snapshot!.overview.progress, review: 1},
      },
    },
  };
  for (const name of ['dark', 'light']) {
    const rows = compactGoals([review], review.focus.id, getThemeByName(name)!);
    const heading = plain(rows.render(160)).split('\n')[0]!;
    assert.match(heading, /^◎ /);
    assert.doesNotMatch(heading, /◇/);
  }
  assert.equal(
    goalCue(review),
    '◇',
    'review still contributes to attention ordering',
  );
});

test('questions beat review badges and urgent non-selected goals stay visible', () => {
  const views = goalFixtures(3);
  const original = views[2]!;
  const question = {
    ...original,
    snapshot: {
      ...original.snapshot!,
      overview: {
        ...original.snapshot!.overview,
        progress: {...original.snapshot!.overview.progress, review: 1},
      },
    },
  };
  views[2] = question;
  views.push({...views[1]!, focus: {...views[1]!.focus, id: 't-extra'}});
  const live = activity(question.focus.project, question.focus.id, 'asking');
  assert.equal(goalCue(question, live), '?');
  const before = structuredClone(views);
  for (const name of ['dark', 'light']) {
    const theme = getThemeByName(name)!;
    const rows = compactGoals(views, views[0]!.focus.id, theme, live);
    const output = plain(rows.render(100));
    assert.match(output, /◎ \? Refresh usage examples/);
    assert.doesNotMatch(output, /Align CLI output/);
    for (const width of [1, 12, 40, 100]) {
      assert.ok(rows.render(width).length <= 6);
      assert.ok(rows.render(width).every(line => visibleWidth(line) <= width));
    }
  }
  assert.deepEqual(views, before);
});

test('worker row projection excludes terminal/unassigned history and marks quiet or asking', () => {
  const assigned = {project: '/workspace/example', id: 't-333333'};
  const snapshot = projectWorkerActivity(
    [
      {assignment: assigned, state: 'running', idleMs: 0},
      {assignment: assigned, state: 'running', idleMs: 90_000},
      {assignment: assigned, state: 'asking', idleMs: 90_000},
      {
        assignment: assigned,
        state: 'running',
        idleMs: 0,
        failed: 'call failed',
      },
      {assignment: assigned, state: 'completed', idleMs: 0},
      {assignment: assigned, state: 'running', idleMs: 0, finished: true},
      {state: 'running', idleMs: 0},
    ],
    4,
    90_000,
  );
  assert.equal(snapshot.frame, 4);
  assert.deepEqual(
    snapshot.workers.map(worker => worker.state),
    ['running', 'quiet', 'asking', 'quiet'],
  );
});

test('persistent rows keep a four-line singleton and six-line multi-follow budget', () => {
  for (const phase of [0, 1, 2, 4, 5, 7, 8, 10, 11, 12, 13]) {
    const views = goalFixtures(phase);
    for (const width of [1, 2, 12, 30, 80, 160]) {
      const lines = compactGoals(
        views,
        views[0]!.focus.id,
        theme,
        goalActivityFixture(phase, 2),
      ).render(width);
      assert.ok(lines.length <= 4, `${phase}/${width}: ${lines.length}`);
      assert.ok(lines.every(line => visibleWidth(line) <= width));
      assert.doesNotMatch(plain(lines), /\uFFFD|\x07/);
    }
  }
  const nested = goalFixtures(11);
  const nestedLines = plain(
    compactGoals(nested, nested[0]!.focus.id, theme).render(160),
  ).split('\n');
  assert.match(nestedLines[0]!, /^◎ /);
  assert.match(nestedLines[1]!, /^ {2}\S /);
  assert.match(nestedLines[2]!, /^ {2}\S /);
  assert.match(nestedLines[3]!, /^ {2}\d+ more visible slices/);
  const complete = goalFixtures(2);
  assert.equal(
    compactGoals(complete, complete[0]!.focus.id, theme).render(80).length,
    1,
  );
  const many = Array.from({length: 16}, (_, index) => ({
    ...goalFixtures(11)[0]!,
    focus: {id: `t-${index.toString(16)}`, project: '/workspace/example'},
  }));
  for (const width of [1, 24, 80, 160]) {
    const lines = compactGoals(many, 't-f', theme).render(width);
    assert.ok(lines.length <= 6);
    assert.ok(lines.every(line => visibleWidth(line) <= width));
  }
  assert.match(
    plain(compactGoals(many, 't-f', theme).render(160)),
    /more followed/,
  );
  const unicode = goalFixtures(11)[0]!;
  const long = {
    ...unicode,
    snapshot: {
      ...unicode.snapshot!,
      overview: {
        ...unicode.snapshot!.overview,
        goal: {
          ...unicode.snapshot!.overview.goal,
          root: {
            ...unicode.snapshot!.overview.goal.root,
            title: '日本語 👩‍💻 '.repeat(80),
          },
        },
      },
    },
  };
  assert.ok(
    compactGoals([long], long.focus.id, theme)
      .render(30)
      .every(line => visibleWidth(line) <= 30),
  );
});
