import assert from 'node:assert/strict';
import test from 'node:test';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';
import {goalFixtures} from '../dev/goals.ts';
import {GoalPanel, panelWidth} from '../extensions/goal/panel.ts';
import {
  AttentionView,
  GoalPopup,
  popupBodyHeight,
  type Reply,
} from '../extensions/goal/attention/panel.ts';
import {attentionModel} from '../extensions/goal/attention/model.ts';
import type {
  AttentionList,
  Request,
} from '../extensions/goal/attention/source.ts';
import {
  PROJECT,
  request as fixtureRequest,
  requestId,
} from './attention-fixture.mjs';

const request = (overrides: Partial<Request> = {}): Request =>
  fixtureRequest(overrides) as Request;

initTheme('dark', false);
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
const bindings = {
  matches: (data: string, action: string) =>
    (data === '\r' && action === 'tui.select.confirm') ||
    (data === '\x1b' && action === 'tui.select.cancel'),
};
const RIGHT = '\x1b[C';
const LEFT = '\x1b[D';
const DOWN = '\x1b[B';
const UP = '\x1b[A';

const list = (requests: Request[]): AttentionList => ({
  goals: [
    {
      project: PROJECT,
      project_name: 'fixture',
      goal: 't-123abc',
      title: 'Fixture goal',
      status: 'open',
      coordinator: null,
      mandate: null,
      requests,
    },
  ],
  errors: [],
});

function harness(rows = 40, theme = 'dark') {
  const answers: [string, Reply][] = [];
  const dismissed: string[] = [];
  const drafts = new Map<string, string>();
  const views: string[] = [];
  let closed = 0;
  const palette = getThemeByName(theme)!;
  const attention = new AttentionView(
    bindings,
    {
      answer: (id, reply) => answers.push([id, reply]),
      dismiss: id => dismissed.push(id),
      drafts,
    },
    () => rows,
    () => {},
  );
  const shown: {popup?: GoalPopup} = {};
  const work = new GoalPanel(
    bindings,
    {close: () => shown.popup?.back(), cycle() {}},
    () => {},
    () => popupBodyHeight(rows),
  );
  work.setGoal({view: goalFixtures(8)[0]!, followed: 1}, palette);
  const popup = new GoalPopup(
    {
      attention,
      work,
      close: () => closed++,
      viewed: view => views.push(view),
    },
    'attention',
  );
  popup.themeSource = () => palette;
  shown.popup = popup;
  const set = (requests: Request[], local = new Map()) =>
    attention.setModel(
      attentionModel(list(requests), undefined, local),
      palette,
    );
  return {
    attention,
    popup,
    answers,
    dismissed,
    drafts,
    views,
    closed: () => closed,
    set,
    render: (width = 120) => plain(popup.render(width)),
  };
}

test('the selected question and its options show immediately', () => {
  const h = harness();
  h.set([
    request(),
    request({id: requestId(2), title: 'Approve the release', created_at: 200}),
  ]);
  const out = h.render();
  assert.match(out, /› \[ ← Attention \(2\) \][\s\S]*\[ Work → \]/);
  assert.match(out, /Needs your attention[\s\S]*2 outstanding/);
  assert.match(out, /Should the first slice cover every project/);
  assert.match(
    out,
    /This project only[\s\S]*Every project[\s\S]*Type a different answer/,
  );
  assert.match(out, /Requests[\s\S]*Choose the rollout scope/);
});

test('choices answer the exact request; typing keeps the question and draft', () => {
  const h = harness();
  h.set([request()]);
  h.popup.handleInput('\r'); // Into the options.
  h.popup.handleInput('\r'); // First option.
  assert.deepEqual(h.answers, [[requestId(1), {choice: 'one'}]]);
  h.popup.handleInput(DOWN);
  h.popup.handleInput(DOWN);
  h.popup.handleInput('\r'); // Type a different answer.
  assert.equal(h.attention.typing, true);
  for (const ch of 'abc') h.popup.handleInput(ch);
  h.popup.handleInput(LEFT);
  h.popup.handleInput('X');
  assert.equal(
    h.drafts.get(requestId(1)),
    'abXc',
    'Left moves the cursor while typing',
  );
  assert.equal(h.popup.current, 'attention');
  const typing = h.render();
  assert.match(
    typing,
    /Should the first slice cover every project/,
    'question retained',
  );
  assert.match(typing, /Answer: abXc/);
  assert.match(typing, /Enter send · Esc back · ←\/→ edit answer/);
  h.popup.handleInput('\x1b'); // Leave the field; the draft stays.
  assert.equal(h.attention.typing, false);
  h.popup.handleInput(RIGHT);
  assert.equal(
    h.popup.current,
    'work',
    'Right switches views outside text entry',
  );
  h.popup.handleInput(LEFT);
  assert.equal(h.popup.current, 'attention');
  h.set([request()]); // A refresh keeps the draft and the chosen option.
  h.popup.handleInput('\r');
  h.popup.handleInput('\r');
  assert.match(h.render(), /Answer: abXc/);
  h.popup.handleInput('\r');
  assert.deepEqual(h.answers.at(-1), [requestId(1), {text: 'abXc'}]);
});

test('Escape steps back, Work returns to Attention, and closing never answers', () => {
  const h = harness();
  h.set([request()]);
  h.popup.handleInput('\r');
  h.popup.handleInput('\x1b');
  assert.equal(h.attention.atTop, true);
  h.popup.handleInput(RIGHT);
  h.popup.handleInput('\x1b');
  assert.equal(
    h.popup.current,
    'attention',
    'Work steps back to outstanding attention',
  );
  h.popup.handleInput('\x1b');
  assert.equal(h.closed(), 1);
  assert.deepEqual(h.answers, []);
  const empty = harness();
  empty.set([]);
  empty.popup.show('work');
  empty.popup.handleInput('\x1b');
  assert.equal(empty.closed(), 1, 'without requests Work closes');
  empty.popup.handleInput('\x1b[103;3u'); // alt+g, kitty form
  assert.ok(empty.closed() >= 1);
});

test('stale and closed records keep the draft beside the replacement', () => {
  const h = harness();
  const original = request();
  const replacement = request({
    id: requestId(2),
    question: 'Which projects now?',
  });
  h.drafts.set(requestId(1), 'only this one');
  h.set(
    [replacement],
    new Map([
      [
        requestId(1),
        {
          state: {kind: 'stale', replacement},
          request: original,
          goal: 'Fixture goal',
        },
      ],
    ]),
  );
  h.attention.setModel(
    attentionModel(
      list([replacement]),
      undefined,
      new Map([
        [
          requestId(1),
          {
            state: {kind: 'stale', replacement},
            request: original,
            goal: 'Fixture goal',
          },
        ],
      ]),
    ),
    getThemeByName('dark')!,
    requestId(1),
  );
  const out = h.render();
  assert.match(out, /Your draft \(not sent\): only this one/);
  assert.match(
    out,
    /This question changed\. New question: Which projects now\?/,
  );
  assert.match(out, /Answer the new question…[\s\S]*Discard my draft/);
  h.popup.handleInput('\r');
  h.popup.handleInput('\r'); // Enter rests on reading context.
  assert.equal(h.attention.selection, requestId(1));
  h.popup.handleInput('\r'); // Back from context.
  h.popup.handleInput(UP);
  h.popup.handleInput(UP);
  h.popup.handleInput('\r'); // Deliberately go to the replacement.
  assert.equal(h.attention.selection, requestId(2));
  assert.doesNotMatch(h.render(), /only this one/, 'no inherited draft');
  assert.deepEqual(
    h.answers,
    [],
    'nothing applies to the replacement automatically',
  );
});

const answered = (overrides: Partial<Request> = {}) =>
  request({
    state: 'answered',
    answer: {
      source: 'ui',
      choice: 'one',
      text: null,
      provenance: 'pi session s · goal popup',
      at: 150,
      by: 'pi-fixture-0000000000000000',
    } as Request['answer'],
    ...overrides,
  });

test('repeated Enter after a save never answers the next question', () => {
  const h = harness();
  const next = request({
    id: requestId(2),
    title: 'Approve the release',
    question: 'Release on Friday?',
    created_at: 50,
  });
  h.set([request(), next]);
  // The next question sorts first; the person deliberately picks the first.
  h.popup.handleInput(DOWN);
  assert.equal(h.attention.selection, requestId(1));
  h.popup.handleInput('\r');
  h.popup.handleInput('\r');
  assert.deepEqual(h.answers, [[requestId(1), {choice: 'one'}]]);
  const mash = () => {
    for (let i = 0; i < 6; i++) h.popup.handleInput('\r');
  };
  // Saved: the answered request moves down the list, the selection does not.
  h.set([answered(), next]);
  mash();
  assert.equal(h.attention.selection, requestId(1));
  assert.match(h.render(), /Answer saved: This project only/);
  // Incorporated: it leaves the store's list but stays selected and inert.
  h.set([next]);
  mash();
  assert.equal(h.attention.selection, requestId(1));
  const out = h.render();
  assert.match(out, /Should the first slice cover every project/);
  assert.match(out, /No longer outstanding/);
  assert.doesNotMatch(out, /Release on Friday\?/);
  assert.equal(h.answers.length, 1, 'the next question was never answered');
  // Moving to the next question is deliberate and drops the retired one.
  h.popup.handleInput('\x1b');
  h.popup.handleInput('\x1b');
  h.popup.handleInput(UP);
  assert.equal(h.attention.selection, requestId(2));
  assert.match(h.render(), /Release on Friday\?/);
  h.popup.handleInput(DOWN);
  assert.equal(h.attention.selection, requestId(2), 'the retired row is gone');
});

test('repeated Enter after a stale save never answers the replacement', () => {
  const h = harness();
  h.set([request()]);
  h.popup.handleInput('\r');
  h.popup.handleInput(DOWN);
  h.popup.handleInput(DOWN);
  h.popup.handleInput('\r'); // Type a different answer.
  for (const key of 'Atlas') h.popup.handleInput(key);
  h.popup.handleInput('\r');
  assert.deepEqual(h.answers, [[requestId(1), {text: 'Atlas'}]]);
  const replacement = request({
    id: requestId(2),
    question: 'Atlas, Beacon or Compass?',
    supersedes: requestId(1),
  });
  h.set(
    [replacement],
    new Map([
      [
        requestId(1),
        {
          state: {kind: 'stale', replacement},
          request: request({state: 'closed', superseded_by: requestId(2)}),
          goal: 'Fixture goal',
        },
      ],
    ]),
  );
  for (let i = 0; i < 6; i++) h.popup.handleInput('\r');
  assert.equal(h.attention.selection, requestId(1));
  assert.equal(h.answers.length, 1, 'the replacement was never answered');
  const out = h.render();
  assert.match(out, /Should the first slice cover every project/);
  assert.match(out, /Your draft \(not sent\): Atlas/);
  assert.match(out, /New question: Atlas, Beacon or Compass\?/);
  assert.equal(h.drafts.get(requestId(1)), 'Atlas');
  assert.equal(h.drafts.get(requestId(2)), undefined);
});

test('mouse: tabs switch views and a clicked option answers', () => {
  const h = harness();
  h.set([request()]);
  const lines = h.popup.render(120).map(line => stripTerminalSequences(line));
  const height = lines.length;
  const mouse = (type: 'press' | 'click', x: number, y: number) =>
    h.popup.handleMouse({
      type,
      button: 'left',
      x,
      y,
      screenX: x,
      screenY: y,
      width: 120,
      height,
      shift: false,
      alt: false,
      ctrl: false,
    });
  const tab = lines[0]!.indexOf('[ Work');
  mouse('press', tab + 1, 0);
  mouse('click', tab + 1, 0);
  assert.equal(h.popup.current, 'work');
  h.popup.show('attention');
  const shown = h.popup.render(120).map(line => stripTerminalSequences(line));
  // The option row sits just above the free-text entry.
  const y =
    shown.findIndex(line => line.includes('Type a different answer')) - 1;
  const x = shown[y]!.indexOf('Every project');
  mouse('press', x, y);
  mouse('click', x, y);
  assert.deepEqual(h.answers, [[requestId(1), {choice: 'all'}]]);
});

test('narrow: clipped choices read in full, labels and details', () => {
  const h = harness(16);
  const keep = 'Migrate every project now, keeping the legacy catalog online';
  const drop = 'Migrate every project now, dropping the legacy catalog today';
  const detail =
    'Keeps old readers working for one release, at the cost of a second write path that must be retired by hand later.';
  h.set([
    request({
      choices: [
        {id: 'keep', label: keep, detail},
        {id: 'drop', label: drop, detail: null},
      ],
    }),
  ]);
  const width = panelWidth(40);
  const clipped = plain(h.popup.render(width));
  assert.doesNotMatch(clipped.replace(/\s+/g, ' '), /legacy catalog online/);
  assert.match(clipped, /Read in full…/);
  h.popup.handleInput('\r');
  for (let i = 0; i < 3; i++) h.popup.handleInput(DOWN);
  h.popup.handleInput('\r');
  const seen: string[] = [];
  for (let i = 0; i < 40; i++) {
    seen.push(plain(h.popup.render(width)));
    h.popup.handleInput(DOWN);
  }
  const text = seen
    .join('\n')
    .split('\n')
    .map(line => line.replace(/^[│\s]+|[│\s]+$/g, ''))
    // Body lines only: the frame and controls repeat on every page.
    .filter(line => !/^[╭╰›\[]/.test(line))
    .filter((line, at, all) => all.indexOf(line) === at)
    .join(' ')
    .replace(/\s+/g, ' ');
  for (const full of [keep, drop, detail])
    assert.ok(text.includes(full), `full text: ${full}`);
  assert.deepEqual(h.answers, [], 'reading answers nothing');
});

test('40x16, light and dark: the frame, question and options fit', () => {
  for (const theme of ['dark', 'light']) {
    const h = harness(16, theme);
    h.set([request()]);
    const width = panelWidth(40);
    const lines = h.popup.render(width);
    assert.ok(lines.length <= 16, `${theme}: ${lines.length} rows`);
    for (const line of lines) assert.ok(visibleWidth(line) <= width);
    const out = plain(lines);
    assert.match(out, /Should the first/);
    assert.match(out, /This project only/);
    assert.match(out, /\[ Close \]/);
  }
});
