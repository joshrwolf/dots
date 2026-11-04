import assert from 'node:assert/strict';
import test from 'node:test';
import {
  CustomEditor,
  ToolExecutionComponent,
  getSelectListTheme,
  initTheme,
  type ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {
  ProcessTerminal,
  TuiMainScreen,
  stripTerminalSequences,
  visibleWidth,
} from '@earendil-works/pi-tui';
import {
  activityDecoration,
  StateGutter,
  type ActivityState,
} from '../src/ui/state.ts';
import {activityHeader, Notices, ActivityCard} from '../src/ui/card.ts';
import {DiffView} from '../src/ui/previews/view.ts';
import {SoftEditor} from '../src/features/editor.ts';
import {duration} from '../src/features/tools/records.ts';
import {toolRenderers} from '../extensions/ui/index.ts';
import {installEditor} from '../src/features/editor.ts';
// Exact-version host fixtures; production imports public APIs only.
import {KeybindingsManager} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/core/keybindings.js';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';

const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
initTheme('dark', false);
const theme = getThemeByName('dark');
assert.ok(theme);

test('ActivityCard uses one explicit header model and preserves body width', () => {
  const card = new ActivityCard();
  const widths: number[] = [];
  const model = {
    header: {
      layout: 'inline',
      start: [
        {
          text: 'write example.ts',
          tone: 'toolTitle',
          priority: 50,
          shrink: 'clip',
        },
      ],
      end: [{text: 'written', tone: 'success', priority: 100, shrink: 'clip'}],
    } as const,
    notices: [],
    caption: 'recorded source',
    body: {
      invalidate() {},
      render(width: number) {
        widths.push(width);
        return ['body'];
      },
    },
  };
  card.update(model, theme);
  assert.equal(
    plain(card.render(240)).split('\n')[0]?.trimEnd(),
    '  write example.ts written',
  );
  card.update({...model, header: {...model.header, layout: 'edges'}}, theme);
  assert.equal(
    visibleWidth(plain(card.render(240)).split('\n')[0]!.trimEnd()),
    236,
  );
  assert.deepEqual(widths, [238, 238]);
});

test('explicit card headers preserve caller ordering and spacing', () => {
  const card = new ActivityCard();
  card.update(
    {
      header: {
        layout: 'inline',
        separator: ' · ',
        groupSeparator: ' · ',
        start: [
          {text: 'read', tone: 'muted', priority: 90, shrink: 'clip', padTo: 5},
          {text: 'done', tone: 'success', priority: 100, shrink: 'clip'},
        ],
        end: [
          {text: 'example.ts', tone: 'toolTitle', priority: 10, shrink: 'clip'},
        ],
      },
      body: undefined,
      notices: [],
      caption: '',
    },
    theme,
  );
  assert.equal(
    plain(card.render(120)).trimEnd(),
    '  read  · done · example.ts',
  );
});

test('short timings are quiet only in collapsed tool summaries', () => {
  const state = {
    kind: 'result',
    text: '',
    partial: false,
    error: false,
    durationMs: 42,
    image: false,
    showImages: false,
  } as const;
  assert.deepEqual(duration(state), []);
  assert.equal(duration(state, true)[0]?.text, '0.0s');
  assert.equal(duration({...state, durationMs: 1000})[0]?.text, '1.0s');
  assert.deepEqual(duration({...state, partial: true}, true), []);
});

test('caption policy follows actual header visibility on every resize', () => {
  const card = new ActivityCard();
  const path = '/workspace/example.ts';
  const model = {
    header: {
      start: [{text: path, tone: 'toolTitle', priority: 10, shrink: 'clip'}],
      end: [],
    },
    caption: path,
    notices: [],
    body: {
      invalidate() {},
      render() {
        return ['SOURCE'];
      },
    },
  } as const;
  card.update({...model, captionPolicy: 'when-not-in-header'}, theme);
  const wide = plain(card.render(100));
  assert.equal(wide.split(path).length - 1, 1);
  assert.equal(wide.split('\n').length, 2); // No empty caption row.
  const narrow = card
    .render(12)
    .slice(1)
    .map(line => stripTerminalSequences(line).replace(/^│ /, '').trim())
    .join('');
  assert.ok(narrow.includes(path));
  assert.equal(plain(card.render(100)), wide);
  card.update(model, theme);
  assert.equal(plain(card.render(100)).split(path).length - 1, 2); // Explicit/default always.
});

test('recorded diffs wrap within the width and recompute after resize', () => {
  const view = new DiffView('-1 old\n+1 日本語', theme);
  const narrow = plain(view.render(80));
  assert.match(narrow, /-1 old\n\+1 日本語/);
  assert.equal(plain(view.render(240)), narrow);
  const long = new DiffView(
    '-1 ' + 'a'.repeat(80) + '\n+1 ' + 'b'.repeat(100),
    theme,
  );
  for (const component of [view, long])
    for (const width of [1, 20, 80, 120, 240])
      assert.ok(
        component.render(width).every(line => visibleWidth(line) <= width),
      );
  assert.match(plain(long.render(40)), /b{30}/);
});

test('state decoration distinguishes activity without color and stays bounded', t => {
  t.mock.method(Date, 'now', () => 0);
  const child = {
    invalidate() {},
    render: (width: number) => ['bash'.slice(0, width)],
  };
  const expected: Record<ActivityState, string> = {
    receiving: '·',
    queued: '◦',
    running: '⠋',
    done: ' ',
    failed: '✗',
    cancelled: '−',
    unknown: '?',
    waiting: '?',
  };
  for (const state of Object.keys(expected) as ActivityState[]) {
    const view: StateGutter = new StateGutter(child, state, theme);
    assert.equal(plain(view.render(20)), `${expected[state]} bash`);
    assert.deepEqual(view.render(20), view.render(20));
    for (const width of [0, 1, 2, 3, 20])
      assert.ok(view.render(width).every(line => visibleWidth(line) <= width));
  }
});

test('only running gutters animate on owner redraws, without allocating timers', t => {
  let now = 0;
  t.mock.method(Date, 'now', () => now);
  const timer = t.mock.method(globalThis, 'setInterval');
  const child = {invalidate() {}, render: () => ['work']};
  const running = new StateGutter(child, 'running', theme);
  const waiting = new StateGutter(child, 'waiting', theme);
  const done = new StateGutter(child, 'done', theme, false);
  const first = plain(running.render(20));
  const stable = [plain(waiting.render(20)), plain(done.render(20))];
  now = 120;
  assert.notEqual(plain(running.render(20)), first);
  assert.deepEqual([plain(waiting.render(20)), plain(done.render(20))], stable);
  assert.equal(timer.mock.callCount(), 0);
});

test('activity colors distinguish work from attention without coloring every success', () => {
  assert.deepEqual(activityDecoration('running'), {glyph: '●', tone: 'accent'});
  assert.deepEqual(activityDecoration('waiting'), {
    glyph: '?',
    tone: 'warning',
  });
  assert.deepEqual(activityDecoration('failed'), {glyph: '✗', tone: 'error'});
  assert.deepEqual(activityDecoration('done'), {glyph: ' ', tone: 'muted'});
  const notices = new Notices();
  notices.update(
    [
      {text: 'Recover the result', severity: 'warning'},
      {text: 'Permission denied', severity: 'error'},
    ],
    theme,
  );
  assert.match(plain(notices.render(80)), /! Recover the result/);
  assert.match(plain(notices.render(80)), /↳ Permission denied/);
});

test('tool subjects have a stable anchor and diagnostics are softly indented', () => {
  const card = new ActivityCard();
  const render = (verb: string, metadata: string) => {
    card.update(
      {
        header: activityHeader(verb, 'example.ts', [
          {text: metadata, tone: 'dim', priority: 50, shrink: 'clip'},
        ]),
        state: 'done',
        notices: [],
        body: undefined,
        caption: '',
      },
      theme,
    );
    return plain(card.render(120));
  };
  const write = render('write', '2 lines');
  const read = render('read', 'lines 1–3');
  assert.equal(write.indexOf('example.ts'), read.indexOf('example.ts'));
  assert.match(write, /write +example\.ts +2 lines/);
  assert.doesNotMatch(write, /·/);
  card.update(
    {
      header: activityHeader('read', 'missing.ts', []),
      state: 'failed',
      notices: [{text: 'File not found', severity: 'error'}],
      body: undefined,
      caption: '',
    },
    theme,
  );
  const lines = card.render(120).map(line => stripTerminalSequences(line));
  assert.match(lines[0]!, /^✗ read/);
  assert.match(lines[1]!, /^  ↳ File not found/);
  assert.doesNotMatch(lines.join(''), /failed/);
  for (const width of [1, 8, 16, 20, 80])
    assert.ok(card.render(width).every(line => visibleWidth(line) <= width));
});

test('file subjects are cwd-relative, full paths stay in expanded details, errors wrap separately', () => {
  const path = '/workspace/packages/example/deep/module.ts';
  const component = new ToolExecutionComponent(
    'read',
    'fixture',
    {path},
    {outputPad: 1},
    toolRenderers().get('read'),
    new TuiMainScreen(new ProcessTerminal()),
    '/workspace',
  );
  component.updateResult({
    content: [{type: 'text', text: 'const value = 1;'}],
    isError: false,
  });
  assert.doesNotMatch(plain(component.render(42)), /\/workspace/);
  assert.match(plain(component.render(42)), /module\.ts/);
  component.setExpanded(true);
  const expanded = component
    .render(30)
    .map(line =>
      stripTerminalSequences(line)
        .replace(/^\s*│ ?/, '')
        .trim(),
    )
    .join('');
  assert.ok(expanded.includes(path));
  component.updateResult({
    content: [
      {
        type: 'text',
        text: 'Permission denied: choose a writable destination.',
      },
    ],
    isError: true,
  });
  component.setExpanded(false);
  assert.match(plain(component.render(60)), /✗ read[\s\S]*Permission denied/);
  for (const width of [1, 3, 20, 42, 80, 140])
    assert.ok(
      component.render(width).every(line => visibleWidth(line) <= width),
    );
});

test('soft editor preserves native text, cursor layout, controls, and semantic border callback', () => {
  const tui = new TuiMainScreen(new ProcessTerminal());
  const bindings = new KeybindingsManager({});
  const editorTheme = {
    borderColor: (text: string) => theme.fg('warning', text),
    selectList: getSelectListTheme(),
  };
  const native = new CustomEditor(tui, editorTheme, bindings, {
    paddingX: 1,
    embedWorkingStatus: true,
  });
  const soft = new SoftEditor(tui, editorTheme, bindings);
  for (const editor of [native, soft]) {
    editor.focused = true;
    editor.setText('日本語 👩🏽‍💻\nsecond line');
    editor.handleInput('\x1b[D');
    editor.handleInput('!');
  }
  assert.equal(soft.getText(), native.getText());
  for (const width of [8, 20, 80]) {
    assert.equal(plain(soft.render(width)), plain(native.render(width)));
    assert.ok(soft.render(width).every(line => visibleWidth(line) <= width));
  }
  for (const width of [1, 2, 3]) {
    assert.equal(plain(soft.render(width)), '…');
    assert.ok(soft.render(width).every(line => visibleWidth(line) <= width));
  }
  assert.equal(soft.getText(), native.getText());
  const mouse = {
    type: 'click' as const,
    button: 'left' as const,
    x: 2,
    y: 1,
    screenX: 2,
    screenY: 1,
    width: 80,
    height: 5,
    shift: false,
    alt: false,
    ctrl: false,
  };
  assert.equal(soft.handleMouse({...mouse, width: 1}), undefined);
  assert.deepEqual(soft.handleMouse(mouse), native.handleMouse(mouse));
  assert.equal(plain(soft.render(80)), plain(native.render(80)));
  const border = (text: string) => theme.fg('success', text);
  soft.borderColor = border;
  assert.doesNotMatch(soft.render(80).join('\n'), /\x1b\[2m/);
  native.borderColor = border;
  assert.deepEqual(soft.render(80), native.render(80));
  assert.equal(soft.borderColor, border);
  let escaped = 0;
  let exited = 0;
  soft.onEscape = () => escaped++;
  soft.handleInput('\x1b');
  soft.setText('');
  soft.onCtrlD = () => exited++;
  soft.handleInput('\x04');
  assert.equal(escaped, 1);
  assert.equal(exited, 1);
  assert.equal(soft.embedWorkingStatus, true);
});

test('editor installation and cleanup respect previous and subsequent owners', () => {
  type Factory = ReturnType<ExtensionContext['ui']['getEditorComponent']>;
  let factory: Factory;
  let installs = 0;
  const host = {
    mode: 'tui' as const,
    ui: {
      getEditorComponent: () => factory,
      setEditorComponent(value: Factory) {
        factory = value;
        installs++;
      },
    },
  };
  const release = installEditor(host);
  assert.equal(installs, 1);
  assert.ok(factory);
  const ours = factory;
  const other: NonNullable<Factory> = (tui, theme, keys) =>
    new CustomEditor(tui, theme, keys);
  factory = other;
  release();
  release();
  assert.equal(factory, other);
  installEditor(host)();
  assert.equal(installs, 1);
  factory = undefined;
  const clean = installEditor(host);
  assert.notEqual(factory, ours);
  clean();
  clean();
  assert.equal(factory, undefined);
  assert.equal(installs, 3);
  installEditor({...host, mode: 'rpc'})();
  assert.equal(installs, 3);
});
