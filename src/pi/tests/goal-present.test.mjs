import assert from 'node:assert/strict';
import test from 'node:test';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';
import {PriorityRow} from '@joshrwolf/pi-kit/ui';
import {compactSummary, inspection} from '../extensions/goal/present.ts';
import {GoalPanel, summaryRow} from '../extensions/goal/panel.ts';
import {orientation} from '../extensions/goal/context.ts';
import {snapshot, task} from './goal-fixture.mjs';
initTheme('dark', false);
const plain = lines => stripTerminalSequences(lines.join('\n'));
const summary = (views, selected, theme) => {
  const row = new PriorityRow();
  row.update(summaryRow(views, selected ?? views[0].focus.id), theme);
  return row;
};
const panel = (view, theme, close = () => {}, bindings = () => false) => {
  const result = new GoalPanel(
    {matches: bindings},
    {close, cycle() {}},
    () => {},
    () => 40,
  );
  result.setGoal({view, followed: 1}, theme);
  return result;
};

test('orientation summarizes maximum evidence sets while explicit inspection retains exact selections', () => {
  const value = snapshot();
  const evidence = Array.from({length: 32}, (_, i) => ({
    slug: `report-${i}`,
    digest: `sha256:${i.toString(16).padStart(64, '0')}`,
  }));
  value.overview.tasks = Array.from({length: 40}, (_, i) =>
    task(`t-${i.toString(16)}`, 'Reviewed slice', {
      parent: value.overview.goal.root.id,
      result_event: i + 1,
      evidence,
      purged_evidence: evidence.slice(0, 1),
    }),
  );
  const model = {
    focus: {id: value.overview.goal.root.id, project: value.project},
    snapshot: value,
  };
  const context = orientation(model);
  assert.doesNotMatch(context, /sha256:/);
  assert.doesNotMatch(context, /pinned evidence selection/);
  assert.match(context, /Results and history: dots task/);
  assert.ok(Buffer.byteLength(context) < 20000);
  const details = inspection(model);
  assert.equal((details.match(/Evidence: report-/g) ?? []).length, 1280);
  assert.match(details, /purged; unavailable/);
});

test('inspection distinguishes contributors and stale owners without leaking the roster into automatic context', () => {
  const value = snapshot();
  const model = {
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
  };
  assert.match(inspection(model), /Worker observations not loaded/);
  value.workers = {
    rows: [],
    truncated: false,
    warning: 'Worker observations unavailable: read failed',
  };
  assert.doesNotMatch(orientation(model), /Worker observations/);
  value.workers = {rows: [], truncated: false};
  assert.match(inspection(model), /not proof that no execution exists/);
  value.workers = {
    rows: [
      {
        id: 'w-fixture',
        parent: 'owner',
        owner_session: '/example/owner.jsonl',
        task_id: 't-456def',
        label: 'writer',
        state: 'running',
        cwd: '/example/checkout',
        updated_at: 1000,
        session_file: '/example/worker.jsonl',
        ownerAvailable: false,
      },
    ],
    truncated: true,
  };
  const rendered = inspection(model);
  assert.match(
    rendered,
    /w-fixture.*task t-456def.*recorded running.*stale: owner process unavailable/,
  );
  assert.match(rendered, /Recorded checkout: \/example\/checkout/);
  assert.match(rendered, /owner session: \/example\/owner.jsonl/);
  assert.match(rendered, /Worker observations limited/);
  assert.match(rendered, /native wt list\/Git/);
});

test('goal views use shared components, fit widths and sanitize terminal controls', () => {
  const value = snapshot(
    't-123abc',
    'cafe\u0301 日本語 👩‍💻 \x1b]52;c;payload\x07',
  );
  const model = {
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
    brief: {text: '# Pattern\n\x1b[2JUse a shared interface.'},
  };
  for (const name of ['dark', 'light']) {
    const theme = getThemeByName(name);
    const widget = summary([model], undefined, theme);
    let closes = 0;
    const inspector = panel(
      model,
      theme,
      () => closes++,
      (key, action) => key === '\r' && action === 'tui.select.confirm',
    );
    for (const width of [1, 2, 10, 40, 80, 160]) {
      for (const view of [widget, inspector]) {
        const lines = view.render(width);
        assert.ok(
          lines.every(line => visibleWidth(line) <= width),
          `${name} ${width}`,
        );
        assert.doesNotMatch(plain(lines), /\x1b|\x07|payload/);
      }
    }
    assert.doesNotMatch(
      plain(widget.render(160)),
      /focus only|0 held|0 dropped|PR recorded/,
    );
    assert.match(plain(widget.render(160)), /0\/1/);
    inspector.handleInput('\r');
    assert.equal(closes, 1);
  }
});

test('goal overview distinguishes claims, dependencies, held work and history without exposing bookkeeping', () => {
  const value = snapshot();
  value.overview.tasks = [
    task('t-aa', 'Reference', {owner: 'fixture'}),
    task('t-bb', 'Consumer', {blocked_by: ['t-aa']}),
    task('t-cc', 'Decision', {status: 'held'}),
    task('t-dd', 'Released'),
  ];
  value.overview.graph = {
    rows: [
      {task: 't-aa', lines: ['●', '│']},
      {task: 't-bb', lines: ['◌']},
      {task: 't-cc', lines: ['!']},
      {task: 't-dd', lines: ['○']},
    ],
    notes: [],
  };
  value.overview.progress = {
    total: 5,
    open: 3,
    done: 1,
    held: 1,
    dropped: 0,
    claimed: 1,
    ready: 1,
    review: 0,
  };
  value.overview.hidden_closed = 1;
  const model = {
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
    brief: {text: '# Accepted design\n\nUse **one** interface.'},
  };
  const theme = getThemeByName('dark');
  const line = () => plain(summary([model], undefined, theme).render(200));
  assert.match(line(), /1\/5 done · 1 held · partial · now: Reference/);
  assert.equal(summary([model], undefined, theme).render(160).length, 1);
  const browser = panel(model, theme);
  const tree = plain(browser.render(120));
  assert.match(tree, /○ Open +Released/);
  assert.match(tree, /→ ● Claimed +Reference · fixture/);
  assert.match(tree, /Claimed by fixture/);
  assert.doesNotMatch(tree, /Project:|Recorded at:|t-aa|Brief content:/);
  assert.match(tree, /1 closed hidden/);
  browser.handleInput('\x1b[B');
  assert.match(plain(browser.render(120)), /Needs: Reference · open/);
  value.overview.progress.claimed = 2;
  assert.doesNotMatch(
    line(),
    /Current:|Running|worker/,
    'multiple claims must not invent a running slice',
  );
  assert.equal(
    compactSummary({...model, error: 'offline'}),
    'unavailable · /goal refresh',
  );
  let redraws = 0;
  const inspector = new GoalPanel(
    {matches: () => false},
    {close() {}, cycle() {}},
    () => redraws++,
    () => 20,
  );
  inspector.setGoal({view: model, followed: 1}, theme);
  assert.doesNotMatch(plain(inspector.render(120)), /Accepted design|Project:/);
  inspector.handleInput('2');
  assert.match(plain(inspector.render(120)), /Accepted design/);
  assert.doesNotMatch(plain(inspector.render(120)), /\*\*one\*\*/);
  inspector.handleInput('3');
  assert.match(plain(inspector.render(120)), /Project:/);
  inspector.handleInput('1');
  assert.match(plain(inspector.render(120)), /◌ Waiting +Consumer/);
  assert.equal(redraws, 4, 'setGoal and each key request one render');
});

test('followed overview keeps selection visible and reports display omissions without hiding context', () => {
  const views = Array.from({length: 8}, (_, n) => {
    const value = snapshot(`t-${n.toString(16)}`, `Goal ${n}`);
    return {
      focus: {id: value.overview.goal.root.id, project: value.project},
      snapshot: value,
      brief: {text: '日本語 👩‍💻'},
    };
  });
  views[0] = {focus: views[0].focus, error: 'Unavailable'};
  for (const name of ['dark', 'light']) {
    const widget = summary(views, 't-7', getThemeByName(name));
    const output = plain(widget.render(160));
    assert.match(output, /Goal 7/);
    assert.match(output, /\+7 followed · needs attention/);
    assert.equal(widget.render(160).length, 1);
    for (const width of [0, 1, 2, 12, 45, 80])
      assert.ok(
        widget.render(width).every(line => visibleWidth(line) <= width),
      );
  }
});

test('singleton brief warnings remain visible after another goal is removed', () => {
  const value = snapshot();
  const healthy = {
    focus: {id: 't-999999', project: value.project},
    snapshot: snapshot('t-999999'),
  };
  for (const warning of [
    'Brief unavailable: removed',
    'Brief truncated; read the reference',
  ]) {
    const view = {
      focus: {id: 't-123abc', project: value.project},
      snapshot: value,
      brief: {text: '', warning},
    };
    const theme = getThemeByName('dark');
    assert.match(
      plain(summary([view, healthy], healthy.focus.id, theme).render(160)),
      /needs attention/,
    );
    assert.match(
      plain(summary([view], view.focus.id, theme).render(160)),
      /brief warning/,
    );
    assert.ok(plain(panel(view, theme).render(160)).includes(warning));
  }
});

test('review results and references remain explicit, bounded, and separate from acceptance', () => {
  const value = snapshot();
  value.overview.goal.root.references = [
    {
      url: 'https://linear.app/example/issue/ENG-123',
      label: 'Goal context',
    },
  ];
  const child = value.overview.tasks[0];
  child.status = 'review';
  child.owner = 'reviewer';
  value.overview.graph.rows[0].lines = ['◇'];
  child.result = 'Checks passed; integration pending. ' + 'x'.repeat(2200);
  child.references = Array.from({length: 10}, (_, n) => ({
    url: `https://github.com/example/project/issues/${n}`,
    label: `Issue ${n}`,
  }));
  value.overview.progress = {
    ...value.overview.progress,
    open: 0,
    review: 1,
    claimed: 1,
    ready: 0,
  };
  const view = {
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
  };
  const content = inspection(view);
  assert.match(content, /Goal context/);
  assert.match(content, /Checks passed; integration pending/);
  assert.match(content, /Result truncated; dots task show/);
  assert.match(content, /2 references omitted/);
  assert.match(
    content,
    /Reference: Issue 0 https:\/\/github.com\/example\/project\/issues\/0/,
  );
  assert.match(content, /1 review/);
  assert.equal(child.references.length, 10);
  const shown = plain(panel(view, getThemeByName('dark')).render(160));
  assert.match(shown, /◇ Review +Reference slice · reviewer/);
  assert.doesNotMatch(shown, /Hierarchy incomplete/);
});

test('orientation limits task detail, retains the brief and reports omissions and unknowns', () => {
  const value = snapshot();
  value.overview.tasks = Array.from({length: 120}, (_, n) =>
    task(`t-${n.toString(16)}`, `slice ${n}`),
  );
  const model = {
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
    brief: {
      text: 'accepted pattern',
      warning: 'Brief truncated; read the reference',
    },
  };
  const content = orientation(model);
  assert.match(content, /80 more unfinished tasks not shown/);
  assert.match(content, /accepted pattern/);
  assert.match(content, /Brief truncated/);
  assert.doesNotMatch(content, /slice 99/);
  const history = inspection({...model, history: true});
  assert.match(
    history,
    /20 tasks omitted; use dots task goal show t-123abc --all --json/,
  );
  assert.match(
    inspection(model),
    /20 tasks omitted; use dots task goal show t-123abc --json/,
  );
  const unavailable = inspection({
    focus: model.focus,
    error: 'CLI unavailable',
  });
  assert.match(unavailable, /CLI unavailable/);
  assert.doesNotMatch(unavailable, /0 done/);
});

test('slice inspection shows only the latest recorded event, labeled as history and bounded', () => {
  const value = snapshot();
  const event = (id, kind, detail, actor = null) => ({
    seq: 3,
    at: 90,
    kind,
    actor,
    detail,
    task: id,
  });
  value.overview.tasks = [
    task('t-aa', 'Held slice', {
      status: 'held',
      last_event: event(
        't-aa',
        'hold',
        'Implemented, uncommitted; awaiting review.',
        'fixture-agent',
      ),
    }),
    task('t-bb', 'Long note', {
      last_event: event(
        't-bb',
        'note',
        'Committed locally. ' + 'y'.repeat(800) + '\x1b]52;c;payload\x07',
      ),
    }),
    task('t-cc', 'Created only', {last_event: event('t-cc', 'created', null)}),
    task('t-dd', 'No event'),
  ];
  const model = {
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
  };
  const content = inspection(model);
  assert.match(
    content,
    /t-aa \[held\] Held slice\n  Latest event \(historical\): 1970-01-01T00:01:30\.000Z · hold · by fixture-agent · Implemented, uncommitted; awaiting review\./,
  );
  assert.match(
    content,
    /Latest event \(historical\): .* · note · Committed locally\. y+\n  \[Event detail truncated; dots task show t-bb --json\]/,
  );
  assert.doesNotMatch(content, /payload|\x1b/);
  assert.match(content, /t-cc \[open\] Created only\nt-dd \[open\] No event\n/);
  assert.equal(content.match(/not a handoff or acceptance/g).length, 1);
  assert.match(
    content,
    /dots task show ID shows the result and recent events; for older events, page dots task events --json --under ID from --since 0\./,
  );
  assert.doesNotMatch(content, /full history/);
  value.overview.tasks = [
    task('t-cc', 'Created only', {last_event: event('t-cc', 'created', null)}),
  ];
  assert.doesNotMatch(inspection(model), /Latest event|recent events/);
  assert.doesNotMatch(
    plain(panel(model, getThemeByName('dark')).render(160)),
    /Latest event/,
  );
});

test('bounded previews stop at grapheme boundaries', () => {
  const value = snapshot();
  const event = (id, detail) => ({
    seq: 3,
    at: 90,
    kind: 'note',
    actor: null,
    detail,
    task: id,
  });
  value.overview.tasks = [
    task('t-aa', 'Emoji', {
      last_event: event('t-aa', 'x'.repeat(511) + '😀tail'),
    }),
    task('t-bb', 'Combining', {
      last_event: event('t-bb', 'x'.repeat(511) + 'e\u0301tail'),
    }),
    task('t-cc', 'Joined', {result: 'x'.repeat(2046) + '👩‍💻tail'}),
  ];
  const content = inspection({
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
  });
  assert.doesNotMatch(
    content,
    /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/,
  );
  assert.match(
    content,
    new RegExp(
      `· note · ${'x'.repeat(511)}\n  \\[Event detail truncated; dots task show t-aa --json\\]`,
    ),
  );
  assert.match(
    content,
    new RegExp(
      `· note · ${'x'.repeat(511)}e\u0301\n  \\[Event detail truncated; dots task show t-bb --json\\]`,
    ),
  );
  assert.match(
    content,
    new RegExp(
      `Result: ${'x'.repeat(2046)}👩‍💻\n\\[Result truncated; dots task show t-cc --json\\]`,
    ),
  );
});

test('worker observations name a missing parent worker accurately', () => {
  const value = snapshot();
  value.workers = {
    rows: [
      {
        id: 'w-direct',
        parent: null,
        owner_session: '/example/owner.jsonl',
        task_id: 't-456def',
        label: 'direct',
        state: 'done',
        cwd: '/example/checkout',
        updated_at: 1000,
        ownerAvailable: true,
      },
    ],
    truncated: false,
  };
  const content = inspection({
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
  });
  assert.match(
    content,
    /Parent worker: none \(spawned by owner session\) · owner session: \/example\/owner.jsonl/,
  );
  assert.doesNotMatch(content, /unknown/);
});

test('a latest event that records the slice result does not repeat it', () => {
  const value = snapshot();
  const result = 'Checks passed; integration pending. ' + 'z'.repeat(2100);
  value.overview.tasks = [
    task('t-aa', 'Reviewed slice', {
      status: 'review',
      result,
      last_event: {
        seq: 4,
        at: 90,
        kind: 'review',
        actor: 'fixture-agent',
        detail: result,
        task: 't-aa',
      },
    }),
  ];
  const content = inspection({
    focus: {id: 't-123abc', project: value.project},
    snapshot: value,
  });
  assert.match(
    content,
    /\n  Latest event \(historical\): 1970-01-01T00:01:30\.000Z · review · by fixture-agent\n/,
  );
  assert.equal(content.match(/Checks passed/g).length, 1);
  assert.doesNotMatch(content, /Event detail truncated/);
  assert.match(content, /Result truncated; dots task show t-aa --json/);
});
