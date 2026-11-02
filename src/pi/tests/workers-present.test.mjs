import assert from 'node:assert/strict';
import test from 'node:test';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';
import {
  STALL_MS,
  createWidget,
  describeActivity,
  describeText,
  projectEvent,
  renderEvent,
} from '../extensions/workers/present.ts';
import {renderWake} from '../extensions/wake/present.ts';

initTheme('dark', false);
const theme = getThemeByName('dark');
const plain = lines => stripTerminalSequences(lines.join('\n'));
// A remapped expand key proves renderers use the host's injected formatters.
const hints = {
  keyText: keybinding => (keybinding === 'app.tools.expand' ? 'alt+x' : ''),
  keyHint: (keybinding, description) =>
    `${hints.keyText(keybinding)} ${description}`,
};

const completed = {
  role: 'custom',
  customType: 'dots.worker.event',
  timestamp: 0,
  display: true,
  content:
    'Worker netpol-api [w-1a2b3c] completed\n\nOUTCOME: policy added\n- api/deploy/networkpolicy.yaml\n- commit 0fabb23\n…\n\n[1,689 chars; full text: worker_status { id: "w-1a2b3c" }]',
  details: {
    id: 'w-1a2b3c',
    kind: 'completed',
    label: 'netpol-api',
    truncated: true,
    elapsed: '2m',
    tokens: 15410,
    session: '/tmp/s.jsonl',
  },
};

test('worker event projects to a card: outcome first, pointer to the full text, recoverable when narrow', () => {
  const model = projectEvent(completed);
  assert.equal(model.heading[0].text, 'worker');
  assert.equal(model.heading[1].text, 'netpol-api');
  assert.equal(model.status[0].text, 'completed');
  assert.match(model.body, /^OUTCOME: policy added/);
  assert.ok(model.notices.some(n => /worker_status/.test(n.text)));

  const collapsedLines = renderEvent(
    completed,
    {expanded: false, outputPad: 1, ...hints},
    theme,
  ).render(100);
  const collapsed = plain(collapsedLines);
  assert.match(
    stripTerminalSequences(collapsedLines[0]),
    /worker +netpol-api +completed/,
    'identity and outcome share the same header contract as tools',
  );
  assert.match(
    collapsed,
    /│ OUTCOME: policy added/,
    'result preview sits under the header',
  );
  assert.match(
    collapsed,
    /alt\+x to expand/,
    'long results advertise expansion',
  );
  const expanded = plain(
    renderEvent(
      completed,
      {expanded: true, outputPad: 1, ...hints},
      theme,
    ).render(100),
  );
  assert.match(expanded, /commit 0fabb23/);
  assert.match(expanded, /session \/tmp\/s\.jsonl/);

  for (const width of [12, 20, 24, 40]) {
    const lines = renderEvent(
      completed,
      {expanded: false, outputPad: 0, ...hints},
      theme,
    ).render(width);
    assert.ok(lines.length > 0);
    for (const line of lines)
      assert.ok(
        stripTerminalSequences(line).length <= width,
        `width ${width}: ${stripTerminalSequences(line)}`,
      );
    if (width >= 20)
      assert.match(plain(lines), /completed/, `state survives width ${width}`);
  }
  // At hopeless widths, the outcome takes priority over identity.
  assert.match(
    stripTerminalSequences(
      renderEvent(
        completed,
        {expanded: false, outputPad: 0, ...hints},
        theme,
      ).render(12)[0],
    ),
    /completed/,
  );
});

test('asking and undeliverable events carry their recovery notice; control characters never render', () => {
  const asking = {
    ...completed,
    content: 'Worker x [w-9] asking\n\nWhich base?\x1b[31m\x07',
    details: {id: 'w-9', kind: 'asking', label: 'x\x1b[2Jbad'},
  };
  const out = plain(
    renderEvent(
      asking,
      {expanded: false, outputPad: 0, ...hints},
      theme,
    ).render(80),
  );
  assert.match(out, /answer with message \{ to: "w-9"/);
  assert.match(out, /Which base\?/);
  assert.doesNotMatch(out, /\x07|\[2J/);
  const lost = {
    ...completed,
    details: {id: 'w-9', kind: 'undeliverable', label: 'x'},
  };
  assert.match(
    plain(
      renderEvent(
        lost,
        {expanded: false, outputPad: 0, ...hints},
        theme,
      ).render(80),
    ),
    /could not be routed/,
  );
});

test('activity text says what a call is doing, bounded and sanitized', () => {
  assert.equal(
    describeActivity('bash', {command: 'git status\x1b[2J'}),
    'bash git status',
  );
  assert.equal(
    describeActivity('edit', {path: 'api/x.yaml', edits: []}),
    'edit api/x.yaml',
  );
  assert.equal(
    describeActivity('grep', {pattern: 'foo', path: 'src'}),
    'grep foo in src',
  );
  assert.equal(
    describeActivity('message', {to: 'parent', text: '?'}),
    'message to parent',
  );
  assert.equal(
    describeActivity('custom', {anything: 'x'.repeat(500)}).length,
    'custom '.length + 200,
  );
  assert.equal(describeActivity('ls', undefined), 'ls');
  assert.equal(
    describeText({
      role: 'assistant',
      content: [
        {type: 'thinking', thinking: 'hm'},
        {type: 'text', text: '\n\nAdding the policy.\nThen tests.'},
      ],
    }),
    'Adding the policy.',
  );
  assert.equal(
    describeText({role: 'user', content: [{type: 'text', text: 'no'}]}),
    '',
  );
});

test('widget rows keep label, state and elapsed without detailed counters', () => {
  const rows = () => [
    {
      label: 'netpol-api',
      state: 'running',
      depth: 0,
      elapsed: '2m',
      tokens: 15410,
      calls: 12,
      detail: 'bash git status',
      idleMs: 1000,
    },
    {
      label: 'web',
      state: 'asking',
      depth: 0,
      elapsed: '40s',
      tokens: 2000,
      calls: 3,
      detail: 'asking: Which ingress?',
      idleMs: 1000,
    },
  ];
  const widget = createWidget(rows, theme);
  const lines = widget.render(120).map(l => stripTerminalSequences(l));
  assert.equal(lines.length, 5);
  assert.match(lines.shift(), /^◇ Workers ─+ \/workers/);
  assert.match(lines[0], /^  ⠋ netpol-api.*2m/);
  assert.match(lines[1], /^    └ bash git status\s*$/);
  assert.match(lines[2], /^  \? web.*40s/);
  assert.match(lines[3], /^    └ asking: Which ingress\?\s*$/);
  assert.doesNotMatch(lines.join('\n'), /tok|calls/);
  const narrow = plain(widget.render(30));
  assert.match(narrow, /netpol-api/);
  assert.match(narrow, /web/);
  assert.doesNotMatch(
    narrow,
    /tok|calls/,
    'counts are the first optional segments to go',
  );
  assert.deepEqual(
    createWidget(() => [], theme).render(80),
    [],
    'nothing live, nothing drawn',
  );
});

test('worker label columns use terminal cells, not string lengths', () => {
  const rows = ['日本語', '👩🏽‍💻', 'review'].map(label => ({
    label,
    state: 'running',
    depth: 0,
    elapsed: '1m',
    tokens: 1,
    calls: 1,
    detail: 'read report.ts',
    idleMs: 0,
  }));
  const lines = createWidget(() => rows, theme)
    .render(100)
    .slice(1)
    .map(stripTerminalSequences)
    .filter(line => line.includes('read report.ts'));
  const columns = lines.map(line =>
    visibleWidth(line.slice(0, line.indexOf('read report.ts'))),
  );
  assert.ok(columns.every(column => column === columns[0]));
});

test('widget flags a failed last call and a quiet worker', () => {
  const rows = () => [
    {
      label: 'a',
      state: 'running',
      depth: 0,
      elapsed: '5m',
      tokens: 1,
      calls: 4,
      detail: '\u201cRetrying.\u201d',
      failed: 'bash',
      idleMs: 0,
    },
    {
      label: 'b',
      state: 'running',
      depth: 0,
      elapsed: '5m',
      tokens: 1,
      calls: 4,
      detail: 'bash sleep 600',
      idleMs: STALL_MS + 30_000,
    },
    {
      label: 'c',
      state: 'running',
      depth: 0,
      elapsed: '5m',
      tokens: 1,
      calls: 1,
      detail: 'worker_wait w-1',
      idleMs: 0,
    },
    {
      label: 'leaf',
      state: 'running',
      depth: 1,
      elapsed: '4m',
      tokens: 900,
      calls: 1,
      detail: 'bash sleep 25',
      idleMs: 0,
    },
    {
      label: 'twig',
      state: 'asking',
      depth: 2,
      elapsed: '1m',
      tokens: 90,
      calls: 1,
      detail: 'asking: ok?',
      idleMs: 0,
    },
  ];
  const lines = createWidget(rows, theme, 10)
    .render(100)
    .slice(1)
    .map(stripTerminalSequences);
  assert.match(lines[0], /^  \u2717 a +bash failed/);
  assert.match(lines[1], /└ “Retrying\.”/);
  assert.match(lines[2], /^  ⠋ b.*quiet 2m +5m/);
  assert.match(lines[5], /└ worker_wait w-1\s*$/);
  assert.match(
    lines[6],
    /^  \u2937 ⠋ leaf.*4m/,
    'child indented under its owner',
  );
  assert.match(lines[8], /^    \u2937 \? twig/, 'grandchild indented further');
  assert.match(lines[9], /^        └ asking: ok\?\s*$/);
  const folded = createWidget(rows, theme, 3)
    .render(100)
    .map(stripTerminalSequences);
  assert.equal(folded.length, 4);
  assert.match(folded[3], /\+4 more \u00b7 \/workers/);
});

test('wake fire renders note, cadence, exit code, and lateness; failed checks preview their evidence', () => {
  const fire = {
    role: 'custom',
    customType: 'dots.wake',
    timestamp: 0,
    display: true,
    content:
      'Wake k-1 (fire 2 of 4, every 1m)\n\nCI changed; inspect\n\ncheck: gh run list\nexit: 1\nfailure\n\nCancel with wake_cancel { id: "k-1" }.',
    details: {
      id: 'k-1',
      fired: 2,
      limit: 4,
      late: 5 * 60_000,
      every: '1m',
      check: 'gh run list',
      exit: 1,
      text: 'CI changed; inspect',
    },
  };
  const collapsed = plain(
    renderWake(fire, {expanded: false, outputPad: 0, ...hints}, theme).render(
      100,
    ),
  );
  assert.match(collapsed, /wake +CI changed; inspect/);
  assert.match(collapsed, /2\/4 · every 1m/);
  assert.match(collapsed, /exit 1/);
  assert.match(collapsed, /late by 5m/);
  assert.match(collapsed, /alt\+x to expand/);
  const expanded = plain(
    renderWake(fire, {expanded: true, outputPad: 0, ...hints}, theme).render(
      100,
    ),
  );
  assert.match(expanded, /check: gh run list/);
  assert.match(expanded, /failure/);
});

test('a worker row says what it is working toward, then what it is doing, and how full its context is', () => {
  const row = {
    label: 'review',
    state: 'running',
    depth: 0,
    elapsed: '3m',
    tokens: 90000,
    calls: 9,
    detail: 'bash rg -n settle',
    idleMs: 1000,
  };
  const render = (extra, width = 140) =>
    plain(createWidget(() => [{...row, ...extra}], theme).render(width));
  assert.match(
    render({headline: 'Validating against code', context: 34}),
    /⠋ review +Validating against code.*3m.*model \?.*✦ \?.*━━━─────── 34%.*\n    └ bash rg -n settle/,
  );
  assert.match(
    render({headline: 'Validating', phase: 'retrying 2/3: 529 overloaded'}),
    /review +Validating.*\n    └ retrying 2\/3: 529 overloaded/,
    'phase replaces current activity, not intent',
  );
  assert.doesNotMatch(
    render({headline: 'Validating', phase: 'retrying'}),
    /bash rg/,
  );
  const tight = render({headline: 'Validating against code', context: 91}, 40);
  assert.match(tight, /review/);
  assert.match(tight, /91%/, 'context value survives narrow rows');
  assert.doesNotMatch(
    tight,
    /[▁▂▃▄▅▆▇█]/,
    'worker overview omits vertical fills',
  );
  assert.match(
    render({context: 83, contextBasis: 'compact'}),
    /⠋ review.*━━━━━━━━── 83%/,
  );
  for (const width of [16, 30, 60, 140]) {
    const lines = createWidget(
      () => [{...row, context: 83, contextBasis: 'compact'}],
      theme,
    ).render(width);
    assert.ok(lines.every(line => visibleWidth(line) <= width));
  }
  const card = projectEvent({
    ...completed,
    details: {...completed.details, context: 83, contextBasis: 'compact'},
  });
  assert.ok(card.status.some(segment => /compact .*83%/.test(segment.text)));
  assert.match(
    render({checkout: {kind: 'branch', label: 'feature/api'}}),
    /⎇ feature\/api/,
  );
  assert.match(
    render({checkout: {kind: 'directory', label: 'scratch'}}),
    /📁 scratch/,
  );
  for (const width of [1, 12, 24, 40]) {
    const lines = createWidget(
      () => [
        {
          ...row,
          context: 82,
          checkout: {kind: 'branch', label: 'feature/日本語-long'},
        },
      ],
      theme,
    ).render(width);
    assert.ok(lines.every(line => visibleWidth(line) <= width));
  }
});

test('worker activity uses the full row below metadata and stays bounded', () => {
  const rows = [
    {
      label: 'review',
      state: 'running',
      depth: 0,
      elapsed: '2m',
      tokens: 0,
      calls: 1,
      idleMs: 0,
      context: 38,
      checkout: {kind: 'branch', label: 'feature/api'},
      detail: 'read src/日本語/'.repeat(20),
    },
    {
      label: 'child',
      state: 'asking',
      depth: 1,
      elapsed: '40s',
      tokens: 0,
      calls: 0,
      idleMs: 0,
      context: 82,
      detail: 'Which comparison should I use? '.repeat(20),
    },
  ];
  for (const width of [100, 120, 160]) {
    const lines = createWidget(() => rows, theme)
      .render(width)
      .map(stripTerminalSequences);
    for (const index of [2, 4]) {
      assert.equal(visibleWidth(lines[index]), width);
      assert.match(lines[index], /\.\.\./);
    }
  }
  for (const width of [1, 8, 20, 40])
    assert.ok(
      createWidget(() => rows, theme)
        .render(width)
        .every(line => visibleWidth(line) <= width),
    );
});

test('worker metadata columns align across branch and elapsed lengths', () => {
  const base = {
    label: 'review',
    state: 'running',
    depth: 0,
    tokens: 0,
    calls: 1,
    idleMs: 0,
    context: 38,
    detail: 'read src/api.ts',
  };
  const rows = [
    {...base, elapsed: '2m', checkout: {kind: 'branch', label: 'feature/api'}},
    {
      ...base,
      contextBasis: 'compact',
      elapsed: '40s',
      checkout: {kind: 'branch', label: 'fix/schema'},
    },
    {...base, elapsed: '1h12m', checkout: {kind: 'branch', label: '日本語'}},
  ];
  const lines = createWidget(() => rows, theme)
    .render(100)
    .map(stripTerminalSequences);
  const column = (line, text) =>
    visibleWidth(line.slice(0, line.indexOf(text)));
  for (const marker of ['⎇', '▄']) {
    assert.equal(column(lines[1], marker), column(lines[3], marker));
    assert.equal(column(lines[1], marker), column(lines[5], marker));
  }
  assert.equal(column(lines[1], '2m'), column(lines[3], '40s'));
  assert.equal(column(lines[1], '2m'), column(lines[5], '1h12m'));
  assert.doesNotMatch(lines.join('\n'), /compact/);
  for (const name of ['light', 'dark']) {
    const mixed = createWidget(
      () => [rows[0], {...rows[1], context: undefined}, rows[2]],
      getThemeByName(name),
    );
    const headers = mixed.render(100).map(stripTerminalSequences);
    assert.equal(column(headers[1], 'review'), column(headers[3], 'review'));
    assert.equal(column(headers[1], 'review'), column(headers[5], 'review'));
    for (const width of [1, 12, 24, 40, 80, 160])
      assert.ok(mixed.render(width).every(line => visibleWidth(line) <= width));
  }
});

test('worker cards report context and cost beside tokens', () => {
  const model = projectEvent({
    ...completed,
    details: {...completed.details, context: 86.4, cost: 0.1834},
  });
  const texts = model.status.map(segment => segment.text);
  assert.ok(texts.includes('━━━━━━━━━─ 86%') && texts.includes('~$0.18'));
  assert.equal(
    model.status.find(segment => segment.text === '━━━━━━━━━─ 86%').tone,
    'warning',
  );
  assert.ok(
    projectEvent({
      ...completed,
      details: {...completed.details, cost: 0.004},
    }).status.some(s => s.text === '~$0.004'),
    'a total without the billed flag reads as an estimate',
  );
  assert.ok(
    projectEvent({
      ...completed,
      details: {...completed.details, cost: 0.1834, costBilled: true},
    }).status.some(s => s.text === '$0.18'),
    'a provider-billed total is unmarked',
  );
});

test('reasoning summaries yield their heading; other thinking does not', async () => {
  const {thinkingHeading} = await import('../src/features/thinking.ts');
  assert.equal(
    thinkingHeading('**Checking status**\n\nI should look at the tests.'),
    'Checking status',
  );
  assert.equal(thinkingHeading('## Planning the fix'), 'Planning the fix');
  assert.equal(
    thinkingHeading('I wonder whether the socket closes.'),
    undefined,
  );
  assert.equal(thinkingHeading('   '), undefined);
});

test('finished workers retain identity, status icons and separated outcomes', () => {
  const rows = [
    {
      label: 'live',
      state: 'running',
      depth: 0,
      elapsed: '1m',
      tokens: 1,
      calls: 1,
      detail: 'bash make',
      idleMs: 1000,
    },
    {
      label: 'done-one',
      state: 'completed',
      depth: 0,
      elapsed: '3m',
      tokens: 1,
      calls: 1,
      detail: 'OUTCOME: policy added',
      idleMs: 1000,
      finished: true,
    },
    {
      label: 'broke',
      state: 'failed',
      depth: 0,
      elapsed: '20s',
      tokens: 1,
      calls: 1,
      detail: 'provider unavailable',
      idleMs: 1000,
      finished: true,
    },
  ];
  const bold = [];
  const observedTheme = Object.create(theme);
  observedTheme.bold = text => {
    bold.push(stripTerminalSequences(text).trim());
    return theme.bold(text);
  };
  const lines = plain(
    createWidget(() => rows, observedTheme).render(120),
  ).split('\n');
  assert.ok(bold.includes('done-one'), 'completed worker identity stays bold');
  assert.ok(bold.includes('broke'), 'failed worker identity stays bold');
  assert.match(lines[1], /⠋ live/);
  assert.match(lines[3], /✓ done-one +completed · OUTCOME: policy added.*3m/);
  assert.match(lines[4], /✗ broke +failed · provider unavailable.*20s/);
});

test('line budget reserves overflow, never splits a worker, and animates running glyphs only', () => {
  const row = label => ({
    label,
    state: 'running',
    depth: 0,
    elapsed: '1m',
    tokens: 1,
    calls: 1,
    detail: 'bash make',
    idleMs: 1000,
  });
  const three = plain(
    createWidget(() => ['a', 'b', 'c'].map(row), theme).render(100),
  ).split('\n');
  assert.equal(three.length, 7);
  assert.doesNotMatch(three.join('\n'), /more/);
  const many = createWidget(
    () => ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'].map(row),
    theme,
  );
  const lines = plain(many.render(100)).split('\n');
  assert.equal(lines.length, 6);
  assert.match(lines[5], /\+6 more · \/workers/);
  for (const width of [1, 8, 24, 40, 80])
    assert.ok(many.render(width).every(line => visibleWidth(line) <= width));
  let frame = 0;
  const animated = createWidget(
    () => [row('live'), {...row('question'), state: 'asking'}],
    theme,
    6,
    () => frame,
  );
  const before = plain(animated.render(80));
  frame++;
  const after = plain(animated.render(80));
  assert.match(before, /⠋ live/);
  assert.match(after, /⠙ live/);
  assert.equal(
    before.replace('⠋', '⠙'),
    after,
    'only the running glyph changes',
  );
});
