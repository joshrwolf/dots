import assert from 'node:assert/strict';
import {mkdtemp, readFile, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  createEditToolDefinition,
  initTheme,
  ToolExecutionComponent,
} from '@earendil-works/pi-coding-agent';
import {
  rgbColor,
  styleText,
  stripTerminalSequences,
  visibleWidth,
} from '@earendil-works/pi-tui';
import register, {messageRenderers} from '../extensions/ui/index.ts';
import {editRenderers} from './ui-fixture.mjs';
import {DiffView} from '../src/ui/previews/view.ts';
import {sourceText as clean} from '@joshrwolf/pi-kit/ui';

initTheme('dark', false);
const theme = {
  bold: text => `\x1b[1m${text}\x1b[22m`,
  appearance: 'dark',
  fg: (_color, text) => text,
  style: (text, options) => styleText(text, options, 'truecolor'),
  colors: {
    error: rgbColor(220, 70, 70),
    text: rgbColor(220, 220, 220),
    toolSuccessBg: rgbColor(20, 20, 20),
    toolErrorBg: rgbColor(20, 20, 20),
    toolDiffAdded: rgbColor(70, 180, 90),
    toolDiffRemoved: rgbColor(220, 70, 70),
    toolDiffContext: rgbColor(150, 150, 150),
  },
};
// Pi's recorded diff rows: marker, line number, text.
const diff =
  ' 1 const before = 1;\n-2 const old = 2;\n+2 const next = 3;\n+3 const extra = 4;\n 4 const after = 5;';
const result = {
  content: [{type: 'text', text: 'Successfully replaced text in sample.ts.'}],
  details: {diff},
  isError: false,
};
const plain = lines => lines.map(stripTerminalSequences).join('\n');
function row(renderers = editRenderers(), args = {path: 'sample.ts'}) {
  return new ToolExecutionComponent(
    'edit',
    'edit-1',
    args,
    {outputPad: 1},
    renderers,
    {requestRender() {}},
    '/missing/project',
  );
}
function assertFits(lines, width) {
  for (const line of lines)
    assert.ok(
      visibleWidth(line) <= width,
      `${visibleWidth(line)} > ${width}: ${JSON.stringify(line)}`,
    );
}

test('registers only presentation; preserves other custom renderers', () => {
  let resolver;
  const messages = new Map();
  register({
    registerMarkdownTransformer() {},
    on() {},
    registerMessageRenderer(type, renderer) {
      messages.set(type, renderer);
    },
    registerToolRenderer(fn) {
      resolver = fn;
    },
  });
  assert.deepEqual(messages, new Map(messageRenderers()));
  const downstream = {renderShell: 'default', renderCall() {}};
  for (const tool of ['find', 'custom_edit']) {
    assert.equal(
      resolver(tool, () => downstream),
      downstream,
    );
  }
  assert.equal(resolver('edit', () => downstream).renderShell, 'self');
  assert.equal(resolver('write', () => downstream).renderShell, 'self');
});

test('real host component transitions from pending to one compact summary', () => {
  const component = row();
  assert.match(plain(component.render(120)), /edit.*sample\.ts/);
  component.markExecutionStarted();
  component.setArgsComplete();
  component.updateResult(result);
  const collapsed = component.render(140);
  assert.equal(collapsed.filter(line => line.trim()).length, 1);
  assert.match(plain(collapsed), /edit\s+sample\.ts +\+2 −1/);
  assert.doesNotMatch(plain(collapsed), /const old/);
  component.setExpanded(true);
  assert.match(
    plain(component.render(140)),
    /-2 const old = 2;\n.*\+2 const next = 3;\n.*\+3 const extra = 4;/,
  );
  component.setExpanded(false);
  assert.equal(component.render(140).filter(line => line.trim()).length, 1);
});

test('errors remain visible collapsed; expanded errors preserve full details', () => {
  const component = row();
  component.updateResult({
    content: [
      {type: 'text', text: 'Unique match not found\nTry a larger context.'},
    ],
    isError: true,
  });
  assert.match(
    plain(component.render(80)),
    /✗ edit[\s\S]*Unique match not found/,
  );
  component.setExpanded(true);
  assert.match(plain(component.render(80)), /Try a larger context/);
  assert.doesNotMatch(plain(component.render(80)), /applied/);
});

test('a missing diff falls back to recorded output without rereading files', () => {
  for (const details of [{patch: '@@ -1 +1 @@\n-old\n+new'}, undefined]) {
    const component = row();
    component.updateResult({...result, details});
    component.setExpanded(true);
    const rendered = plain(component.render(100));
    assert.match(rendered, /preview unavailable/);
    assert.match(rendered, /old|Successfully replaced/);
  }
});

test('replay requires only recorded arguments/results and invalidation updates themes', () => {
  const component = row();
  component.updateResult(result);
  component.setExpanded(true);
  const first = component.render(140);
  initTheme('light', false);
  component.invalidate();
  const second = component.render(140);
  assert.equal(plain(first), plain(second));
  assert.notDeepEqual(first, second);
  const replay = row();
  replay.updateResult(result);
  replay.setExpanded(true);
  assert.deepEqual(replay.render(140), second);
  initTheme('dark', false);
});

test('all layouts fit tiny to wide widths, Unicode, tabs, controls, and padding', () => {
  const text = '\t漢字 👩‍💻 é \x1b[2J\x1b]0;bad\x07 \u202eend';
  const source = `-1 ${text}\n+1 ${text}!`;
  const component = row(undefined, {path: text + '\nfile.ts'});
  component.updateResult({...result, details: {diff: source}});
  for (const expanded of [false, true]) {
    component.setExpanded(expanded);
    for (const padding of [0, 1]) {
      component.setOutputPad(padding);
      for (const width of [1, 2, 3, 5, 10, 40, 80, 119, 120, 121, 140, 240]) {
        const lines = component.render(width);
        assertFits(lines, width);
        assert.doesNotMatch(lines.join('\n'), /\x1b\[2J|\x1b\]0;|\u202e/);
      }
    }
  }
  assert.equal(clean('a\r\x00b\t'), 'ab    ');
});

test('long lines wrap rather than silently clipping', () => {
  const long = 'identifier '.repeat(30) + 'TAIL';
  const view = new DiffView(`-1 ${long}\n+1 short`, theme);
  const lines = view.render(140);
  assert.match(plain(lines), /TAIL/);
  assertFits(lines, 140);
  assert.strictEqual(view.render(140), lines, 'warm redraw reuses layout');
  view.invalidate();
  assert.notStrictEqual(view.render(140), lines);
});

test('large diffs have an explicit display limit', () => {
  const source = Array.from({length: 2000}, (_, n) => `+${n + 1} line`).join(
    '\n',
  );
  const lines = new DiffView(source, theme).render(80);
  assert.equal(lines.length, 1000);
  assert.match(plain(lines), /display limited/);
});

test('independent call/result slot rendering retains the completed summary', () => {
  const renderers = editRenderers();
  const context = {
    state: {},
    args: {path: 'sample.ts'},
    expanded: false,
    isError: false,
    outputPad: 1,
    invalidate() {},
  };
  const call = plain(
    renderers.renderCall(context.args, theme, context).render(100),
  );
  assert.match(call, /sample\.ts/);
  assert.doesNotMatch(call, /editing|applied/);
  const collapsed = plain(
    renderers
      .renderResult(result, {expanded: false, isPartial: false}, theme, context)
      .render(100),
  );
  assert.match(collapsed, /\+2 −1/);
  const expanded = plain(
    renderers
      .renderResult(result, {expanded: true, isPartial: false}, theme, context)
      .render(100),
  );
  assert.match(expanded, /const next/);
});

test('actual SDK edit result renders multi-edits and leaves execution untouched', async () => {
  const dir = await mkdtemp(path.join(tmpdir(), 'dots-edit-display-'));
  try {
    await writeFile(path.join(dir, 'sample.txt'), 'one\ntwo\nthree\nfour\n');
    const args = {
      path: 'sample.txt',
      edits: [
        {oldText: 'one', newText: 'ONE'},
        {oldText: 'four', newText: 'FOUR'},
      ],
    };
    const tool = createEditToolDefinition(dir);
    const outcome = await tool.execute(
      'actual',
      args,
      undefined,
      undefined,
      {},
    );
    assert.equal(typeof outcome.details.diff, 'string');
    const component = row(undefined, args);
    component.updateResult({...outcome, isError: false});
    component.setExpanded(true);
    assert.match(plain(component.render(140)), /ONE/);
    assert.match(plain(component.render(140)), /FOUR/);
    assert.equal(
      await readFile(path.join(dir, 'sample.txt'), 'utf8'),
      'ONE\ntwo\nthree\nFOUR\n',
    );
  } finally {
    await rm(dir, {recursive: true, force: true});
  }
});
