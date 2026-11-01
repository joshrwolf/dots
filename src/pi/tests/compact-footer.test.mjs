import assert from 'node:assert/strict';
import test from 'node:test';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {registerUI as register} from '../extensions/ui/index.ts';
import {createFooter} from '../src/features/footer.ts';

const theme = {
  bold: text => `\x1b[1m${text}\x1b[22m`,
  fg: (role, text) =>
    `\x1b[${role === 'error' ? 31 : role === 'warning' ? 33 : 90}m${text}\x1b[39m`,
};
function fixture() {
  let usageCalls = 0;
  let unsubscribed = 0;
  let requestRenders = 0;
  let branchChange;
  const state = {
    settings: {},
    leaf: 'one',
    session: 'example',
    branch: 'main',
    usage: {tokens: 2500, contextWindow: 10000, percent: 25},
  };
  const ctx = {
    cwd: '/projects/example',
    model: {id: 'model-a', provider: 'provider', contextWindow: 10000},
    thinkingLevel: 'medium',
    sessionManager: {
      getSessionId: () => state.session,
      getLeafId: () => state.leaf,
    },
    getContextUsage() {
      usageCalls++;
      return state.usage;
    },
  };
  const statuses = new Map();
  const data = {
    getGitBranch: () => state.branch,
    getExtensionStatuses: () => statuses,
    onBranchChange(fn) {
      branchChange = fn;
      return () => {
        unsubscribed++;
      };
    },
  };
  const footer = createFooter(
    ctx,
    theme,
    data,
    () => requestRenders++,
    () => state.settings,
  );
  return {
    ctx,
    state,
    statuses,
    footer,
    branchChange: () => branchChange(),
    stats: () => ({usageCalls, unsubscribed, requestRenders}),
  };
}
const plain = lines => stripTerminalSequences(lines.join('\n'));

test('Anthropic 1M footer measures native compaction budget without a label', () => {
  const {footer, ctx, state} = fixture();
  ctx.model = {id: 'fixture', provider: 'anthropic', contextWindow: 1_000_000};
  state.settings = {
    compaction: {
      modelOverrides: {'anthropic/fixture': {reserveTokens: 700_000}},
    },
  };
  state.usage = {tokens: 250_000, contextWindow: 1_000_000, percent: 25};
  assert.match(plain(footer.render(120)), /━━━━━━━━── 83%/);
  assert.match(plain(footer.render(22)), /83%/);
  assert.doesNotMatch(plain(footer.render(120)), /compact/);
  state.leaf = 'two';
  state.usage = {...state.usage, tokens: 300_000, percent: 30};
  assert.match(plain(footer.render(120)), /━━━━━━━━━━ 100%/);
  state.leaf = 'three';
  state.usage = {...state.usage, tokens: null, percent: null};
  assert.match(plain(footer.render(120)), /\?/);
  ctx.model = {...ctx.model, provider: 'openai-codex'};
  state.usage = {tokens: 250_000, contextWindow: 1_000_000, percent: 25};
  assert.match(plain(footer.render(120)), /25%/);
  assert.doesNotMatch(plain(footer.render(120)), /compact/);
  ctx.model = {...ctx.model, provider: 'anthropic'};
  assert.match(plain(footer.render(120)), /83%/);
  state.settings.compaction.enabled = false;
  assert.match(plain(footer.render(120)), /25%/);
  assert.doesNotMatch(plain(footer.render(120)), /compact/);
});

test('one line shows project/branch, current model, thinking, and context', () => {
  const {footer} = fixture();
  const lines = footer.render(120);
  assert.equal(lines.length, 1);
  assert.match(plain(lines), /example  ⑂ main/);
  assert.match(plain(lines), /model-a  ✦ medium +━━━─────── 25%/);
  assert.equal(visibleWidth(lines[0]), 120);
});

test('optional branch and thinking yield before the project, while context has an explicit floor', () => {
  const {footer, ctx, state} = fixture();
  ctx.cwd = '/projects/example-project';
  state.branch = 'feature/long-branch-name';
  const output = plain(footer.render(34));
  assert.match(output, /example-project/);
  assert.match(output, /model-a/);
  assert.match(output, /25%/);
  assert.doesNotMatch(output, /feature|medium|…|\.\.\./);
  // The percent keeps its place; its bar yields first.
  assert.match(plain(footer.render(8)), /25%/);
  assert.doesNotMatch(plain(footer.render(8)), /━|─{2}/);
});

test('context is cached by recorded state, but branch/model/session changes update', () => {
  const {footer, ctx, state, stats, branchChange} = fixture();
  for (let n = 0; n < 50; n++) {
    footer.invalidate();
    footer.render(120);
  }
  assert.equal(stats().usageCalls, 1);
  state.branch = 'feature';
  branchChange();
  assert.equal(stats().requestRenders, 1);
  assert.match(plain(footer.render(120)), /feature/);
  assert.equal(stats().usageCalls, 1);
  state.leaf = 'two';
  state.usage.percent = 80;
  assert.match(footer.render(120)[0], /\x1b\[33m━━━━━━━━──.*\x1b\[33m80%/);
  assert.equal(stats().usageCalls, 2);
  ctx.model = {...ctx.model, id: 'model-b'};
  assert.match(plain(footer.render(120)), /model-b/);
  assert.equal(stats().usageCalls, 3);
  state.session = 'new-session';
  state.usage = {tokens: null, percent: null, contextWindow: 10000};
  assert.match(plain(footer.render(120)), /────────── \?/);
  assert.equal(stats().usageCalls, 4);
});

test('unknown context is not zero, high usage warns, detached and absent branches are honest', () => {
  const {footer, state, ctx} = fixture();
  state.usage = undefined;
  state.branch = null;
  ctx.model = undefined;
  const noModel = plain(footer.render(120));
  assert.match(noModel, /no model/);
  assert.match(noModel, /────────── \?/);
  assert.doesNotMatch(noModel, /main|0%/);
  state.leaf = 'two';
  state.usage = {tokens: 9500, percent: 95, contextWindow: 10000};
  state.branch = 'detached';
  assert.match(footer.render(120)[0], /\x1b\[31m━━━━━━━━━━.*\x1b\[31m95%/);
  assert.match(plain(footer.render(120)), /detached/);
});

test('extension statuses retain their separate line and colors without control commands', () => {
  const {footer, statuses} = fixture();
  statuses.set('b', '\x1b[33mawaiting input\x1b[39m');
  statuses.set('a', '2 workers\nactive\x1b]0;bad-title\x07');
  const lines = footer.render(120);
  assert.equal(lines.length, 2);
  assert.match(plain(lines), /2 workers active.*awaiting input/);
  assert.match(lines[1], /\x1b\[33mawaiting input/);
  assert.doesNotMatch(lines.join('\n'), /\x1b\]0;/);
});

test('status-only updates appear without invalidation or a new session leaf', () => {
  const {footer, statuses, stats} = fixture();
  assert.equal(footer.render(100).length, 1);
  statuses.set('worker', 'awaiting input');
  assert.match(plain(footer.render(100)), /awaiting input/);
  statuses.set('worker', 'finished');
  assert.match(plain(footer.render(100)), /finished/);
  statuses.clear();
  assert.equal(footer.render(100).length, 1);
  assert.equal(stats().usageCalls, 1);
});

test('tiny widths and Unicode stay bounded; context is prioritized', () => {
  const {footer, ctx, state} = fixture();
  ctx.cwd = '/projects/漢字👩‍💻\nexample';
  state.branch = 'branch\x1b[2J';
  for (const width of [1, 2, 3, 5, 10, 20, 40, 80, 120]) {
    for (const line of footer.render(width)) {
      assert.ok(visibleWidth(line) <= width);
      assert.doesNotMatch(line, /\n|\x1b\[2J/);
    }
  }
  assert.match(plain(footer.render(20)), /25%/);
});

test('dispose unsubscribes once and stops rendering stale session context', () => {
  const {footer, stats} = fixture();
  footer.dispose();
  footer.dispose();
  assert.equal(stats().unsubscribed, 1);
  assert.deepEqual(footer.render(120), []);
  assert.equal(stats().usageCalls, 0);
});

test('registration only installs frame surfaces in TUI mode, not RPC/print/JSON', () => {
  let start;
  register({
    registerCommand() {},
    registerMarkdownTransformer() {},
    registerToolRenderer() {},
    on(event, fn) {
      if (event === 'session_start') start = fn;
    },
  });
  let installed = 0;
  let headers = 0;
  const ctx = {
    ui: {
      setHiddenThinkingLabel() {},
      setHeader(factory) {
        assert.equal(typeof factory, 'function');
        headers++;
      },
      getEditorComponent() {
        return () => {};
      },
      setFooter(factory) {
        assert.equal(typeof factory, 'function');
        installed++;
      },
    },
  };
  for (const mode of ['rpc', 'print', 'json']) start({}, {...ctx, mode});
  assert.equal(installed, 0);
  assert.equal(headers, 0);
  start({}, {...ctx, mode: 'tui'});
  assert.equal(installed, 1);
  assert.equal(headers, 1);
});
