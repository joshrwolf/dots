import assert from 'node:assert/strict';
import test from 'node:test';
import {
  initTheme,
  type ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {
  stripTerminalSequences,
  visibleWidth,
  type TUI,
} from '@earendil-works/pi-tui';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';
import {PriorityRow} from '@joshrwolf/pi-kit/ui';
import {summaryRow} from '../extensions/goal/summary.ts';
import {
  JobsWidget,
  jobsStatus,
  projectJob,
} from '../extensions/jobs/present.ts';
import {createWidget} from '../extensions/workers/present.ts';
import {WorkerInspection} from '../extensions/workers/inspection.ts';
import {
  WorkerInspector,
  type WorkerInspectorHost,
} from '../extensions/workers/inspector.ts';
import type {TranscriptOptions} from '../extensions/workers/transcript.ts';
import {FooterView} from '../src/features/frame.ts';
import {renderStatusRow, statusItems} from '../src/ui/status-row.ts';
import {goalFixtures} from '../dev/goals.ts';
import {jobFixture} from '../dev/scenarios.ts';
import {SessionFixture, assistantMessage} from '../dev/sessions.ts';

initTheme('dark', false);
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
const transcript: TranscriptOptions = {
  outputPad: 1,
  hideThinkingBlock: false,
  showImages: false,
  imageWidthCells: 60,
};
const inspectorHost = (
  height: () => number,
  requestRender: () => void = () => {},
): WorkerInspectorHost => ({
  tui: {requestRender} as unknown as TUI,
  theme: getThemeByName('dark')!,
  bindings: {matches: () => false},
  close: () => {},
  height,
  transcript,
});

test('combined persistent sections share unboxed chrome and a bounded default footprint', () => {
  for (const name of ['dark', 'light']) {
    const theme = getThemeByName(name)!;
    const goals = new PriorityRow();
    const views = goalFixtures(8);
    goals.update(summaryRow(views, views[0]!.focus.id), theme);
    const workers = Array.from({length: 9}, (_, i) => ({
      label: `worker-${i}`,
      state: i ? 'running' : 'asking',
      depth: 0,
      elapsed: '1m',
      tokens: 1,
      calls: 1,
      detail: i ? 'read report.ts' : 'Which revision?',
      idleMs: 0,
    }));
    const workerView = createWidget(() => workers, theme);
    const models = [
      projectJob(jobFixture(1)),
      projectJob(jobFixture(5)),
      ...Array.from({length: 5}, () => projectJob(jobFixture(1))),
    ];
    const jobs = new JobsWidget();
    jobs.update(models, theme);
    const footer = new FooterView();
    footer.update(
      {
        project: 'example',
        model: 'example-model',
        contextPercent: 25,
        statuses: [
          {key: 'dots.jobs', text: jobsStatus(models)!},
          {
            key: 'example.notice',
            text: 'Companion update · /reload when idle',
          },
        ],
      },
      theme,
    );
    for (const width of [1, 8, 24, 44, 80, 160]) {
      const lines = [goals, workerView, jobs, footer].flatMap(component =>
        component.render(width),
      );
      assert.ok(lines.length <= 20, `${width}: ${lines.length} lines`);
      assert.ok(lines.every(line => visibleWidth(line) <= width));
      assert.doesNotMatch(plain(lines), /[╭╮╰╯█▓▒░]|\uFFFD/);
    }
    const full = plain(
      [goals, workerView, jobs, footer].flatMap(component =>
        component.render(160),
      ),
    );
    assert.match(full, /◎ ! Unify/);
    assert.match(full, /◇ Workers/);
    assert.match(full, /▷ Jobs/);
    assert.match(full, /Which revision/);
    assert.match(full, /\+7 more · \/workers/);
    assert.match(full, /\+5 more · \/jobs/);
    assert.match(full, /6 active · 1 unknown/);
    for (const component of [goals, workerView, jobs]) {
      const lines = component.render(80).map(stripTerminalSequences);
      if (component !== goals) {
        assert.match(lines[0]!, / ─+ \/(?:workers|jobs)$/);
        assert.ok(lines.slice(1).every(line => line.startsWith('  ')));
      }
      assert.equal(visibleWidth(lines[0]!), 80);
    }
    assert.equal(goals.render(80).length, 1, 'goals use one line');
    assert.match(plain(goals.render(80)), /^◎ ! Unify.* ⌘G$/);
    assert.doesNotMatch(plain(workerView.render(160)), /\d+ calls?|\d+ tok/);
    assert.ok(
      plain(jobs.render(160)).indexOf('unknown') <
        plain(jobs.render(160)).indexOf('running'),
    );
    jobs.update([projectJob(jobFixture(3))], theme);
    assert.deepEqual(jobs.render(160), []);
    assert.equal(jobsStatus([projectJob(jobFixture(3))]), undefined);
    assert.deepEqual(createWidget(() => [], theme).render(160), []);
    // Finishing execution never implicitly unfollows a goal.
    const finished = goalFixtures(2);
    goals.update(summaryRow(finished, finished[0]!.focus.id), theme);
    assert.match(plain(goals.render(160)), /4\/4/);
    assert.equal(goals.render(160).length, 1);
  }
});

test('keyed status summaries deduplicate, preserve colors, and make overflow explicit', () => {
  const items = statusItems([
    {key: 'z', text: 'opaque publisher\x1b]0;unsafe\x07'},
    {key: 'notice', text: 'old'},
    {key: 'notice', text: '\x1b[33mReview requested\x1b[39m'},
    {key: 'dots.jobs', text: 'jobs 1 active'},
    {key: 'empty', text: ''},
  ]);
  assert.deepEqual(
    items.map(item => item.key),
    ['dots.jobs', 'notice', 'z'],
  );
  assert.match(renderStatusRow(items, 100)[0]!, /\x1b\[33m/);
  assert.doesNotMatch(plain(renderStatusRow(items, 100)), /old|unsafe/);
  for (let width = 0; width < 160; width++)
    assert.ok(
      renderStatusRow(items, width).every(line => visibleWidth(line) <= width),
    );
  assert.match(plain(renderStatusRow(items, 25)), /jobs 1 active.*\+2 more/);
  assert.match(plain(renderStatusRow(items, 8)), /\+3/);
});

test('worker inspector bounds public RPC text deltas and keeps its live view open', async () => {
  let renders = 0;
  const worker = {
    id: 'w-example',
    label: 'Review',
    state: 'running',
    depth: 0,
    canMessage: false,
    canStop: false,
  };
  const view = new WorkerInspector(
    {
      list: () => [worker],
      read: () => ({
        worker,
        detail: 'Directory: /fixture/review',
      }),
      message: async () => {},
      stop: async () => {},
    },
    inspectorHost(
      () => 24,
      () => {
        renders++;
      },
    ),
    worker.id,
  );
  try {
    view.render(100);
    view.record(worker.id, {
      type: 'message_start',
      message: assistantMessage([]),
    });
    view.record(worker.id, {
      type: 'message_update',
      assistantMessageEvent: {type: 'text_start', contentIndex: 0},
    });
    view.record(worker.id, {
      type: 'message_update',
      assistantMessageEvent: {
        type: 'text_delta',
        contentIndex: 0,
        delta: 'LIVE_FROM_RPC',
      },
    });
    assert.match(plain(view.render(100)), /LIVE_FROM_RPC/);
    assert.ok(renders > 0);
    view.record(worker.id, {
      type: 'message_end',
      message: assistantMessage([{type: 'text', text: 'LIVE_FROM_RPC'}]),
    });
    assert.match(
      plain(view.render(100)),
      /LIVE_FROM_RPC/,
      'final response remains visible before a session write',
    );
  } finally {
    view.dispose();
  }
});

test('worker picker resizes its native row budget and keeps selection inside one frame', () => {
  const workers = Array.from({length: 40}, (_, index) => ({
    id: `w-${index}`,
    label: `worker-${index}`,
    state: 'running',
    depth: 0,
    canMessage: false,
    canStop: false,
  }));
  let height = 30;
  const view = new WorkerInspector(
    {
      list: () => workers,
      read: id => ({
        worker: workers.find(worker => worker.id === id)!,
        detail: id,
      }),
      message: async () => {},
      stop: async () => {},
    },
    inspectorHost(() => height),
  );
  try {
    const large = view.render(80).map(stripTerminalSequences);
    assert.equal(large.length, 32);
    assert.equal(large.filter(line => line.startsWith('╭')).length, 1);
    assert.match(large.join('\n'), /worker-25/);
    for (let index = 0; index < 18; index++) view.handleInput('\x1b[B');
    height = 6;
    const small = view.render(80).map(stripTerminalSequences);
    assert.equal(small.length, 8);
    assert.match(small.join('\n'), /worker-18/);
    assert.doesNotMatch(small.join('\n'), /worker-0\b/);
    height = 30;
    assert.match(plain(view.render(80)), /worker-18/);
    view.handleInput('\r');
    assert.equal(view.selectedId, 'w-18');
    assert.match(plain(view.render(80)), /Worker · worker-18/);
    assert.equal(
      view
        .render(80)
        .map(stripTerminalSequences)
        .filter(line => line.startsWith('╭')).length,
      1,
    );
  } finally {
    view.dispose();
  }
});

test('worker transcript pins input and action status inside its single frame', async () => {
  const worker = {
    id: 'w-fixture',
    label: 'Review',
    state: 'running',
    depth: 0,
    canMessage: true,
    canStop: true,
  };
  const sent: string[] = [];
  let height = 8;
  const directory = mkdtempSync(path.join(tmpdir(), 'dots-worker-view-'));
  const session = new SessionFixture(
    path.join(directory, 'review.jsonl'),
    '/fixture/review',
  );
  for (let i = 0; i < 60; i++)
    session.assistant([{type: 'text', text: `observation ${i}`}]);
  const view = new WorkerInspector(
    {
      list: () => [worker],
      read: () => ({
        worker,
        detail: '/fixture/review',
        sessionFile: session.file,
      }),
      message: async (_id, text) => {
        sent.push(text);
      },
      stop: async () => {},
    },
    inspectorHost(() => height),
    worker.id,
  );
  try {
    view.focused = true;
    view.render(80);
    for (const key of ['F', 'f']) {
      view.handleInput('\x1b[A');
      assert.match(plain(view.render(80)), /Paused/);
      view.handleInput(key);
      assert.match(
        plain(view.render(80)),
        /Following · ↑\/↓ scroll · F follow/,
      );
    }
    view.handleInput('m');
    const composing = view.render(80).map(stripTerminalSequences);
    assert.equal(composing.length, 10);
    assert.equal(composing.filter(line => line.startsWith('╭')).length, 1);
    assert.match(composing.join('\n'), /Message:|Enter send/);
    assert.ok(
      composing.findIndex(line => line.includes('Message:')) <
        composing.findIndex(line => line.startsWith('╰')),
    );
    height = 1;
    assert.ok(view.render(1).every(line => visibleWidth(line) <= 1));
    assert.ok(
      view.render(1).length <= 3,
      'input keeps the only body row on tiny terminals',
    );
    height = 8;
    for (const key of ['f', 'F', 'h', 'e', 'l', 'l', 'o', '\r'])
      view.handleInput(key);
    await new Promise<void>(resolve => setImmediate(resolve));
    assert.deepEqual(
      sent,
      ['fFhello'],
      'F/f remain literal in the message input',
    );
    assert.match(plain(view.render(80)), /Message delivered/);
  } finally {
    view.dispose();
    rmSync(directory, {recursive: true, force: true});
  }
});

test('worker inspection uses the shared overlay and cannot reopen after shutdown', async () => {
  const inspection = new WorkerInspection();
  let opens = 0;
  let done: () => void = () => {};
  const ctx = {
    mode: 'tui',
    ui: {
      custom: (
        factory: Parameters<ExtensionContext['ui']['custom']>[0],
        options: Parameters<ExtensionContext['ui']['custom']>[1],
      ) => {
        opens++;
        assert.equal(options?.overlay, true);
        return new Promise<void>((resolve, reject) => {
          done = resolve;
          const tui = {terminal: {rows: 40}, requestRender() {}};
          const bindings = {matches: () => false};
          Promise.resolve(
            factory(
              tui as never,
              getThemeByName('dark')!,
              bindings as never,
              () => resolve(),
            ),
          ).catch(reject);
        });
      },
    },
  } as unknown as ExtensionContext;
  const source = {
    list: () => [
      {
        id: 'w-fixture',
        label: 'Review',
        state: 'running',
        depth: 0,
        canMessage: false,
        canStop: false,
      },
    ],
    read: () => ({
      worker: {
        id: 'w-fixture',
        label: 'Review',
        state: 'running',
        depth: 0,
        canMessage: false,
        canStop: false,
      },
      detail: 'Directory: /fixture/review',
    }),
    message: async () => {},
    stop: async () => {},
  };
  const first = inspection.open(ctx, source, transcript);
  await inspection.open(ctx, source, transcript);
  assert.equal(opens, 1);
  done();
  await first;
  const next = inspection.open(ctx, source, transcript, 'w-fixture');
  inspection.dispose();
  await next;
  await inspection.open(ctx, source, transcript);
  assert.equal(opens, 2);
});

test('worker inspector offers a Rex view only when its source can open one', async () => {
  const worker = {
    id: 'w-fixture',
    label: 'Review',
    state: 'running',
    depth: 0,
    canMessage: false,
    canStop: false,
  };
  const source = {
    list: () => [worker],
    read: () => ({worker, detail: 'Directory: /fixture/review'}),
    message: async () => {},
    stop: async () => {},
  };
  const local = new WorkerInspector(
    source,
    inspectorHost(() => 20),
  );
  try {
    assert.doesNotMatch(plain(local.render(120)), /open in Rex/);
    local.handleInput('r');
    assert.doesNotMatch(plain(local.render(120)), /Opened in Rex/);
  } finally {
    local.dispose();
  }
  const opened: string[] = [];
  for (const id of [undefined, worker.id]) {
    const view = new WorkerInspector(
      {
        ...source,
        openInRex: async (target: string) => {
          opened.push(target);
        },
      },
      inspectorHost(() => 20),
      id,
    );
    try {
      assert.match(plain(view.render(120)), /R open in Rex/);
      view.handleInput('R');
      await new Promise<void>(resolve => setImmediate(resolve));
      assert.match(plain(view.render(120)), /Opened in Rex/);
    } finally {
      view.dispose();
    }
  }
  assert.deepEqual(
    opened,
    [worker.id, worker.id],
    'from the list and the transcript',
  );
});
