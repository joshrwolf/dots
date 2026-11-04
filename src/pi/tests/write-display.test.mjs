import assert from 'node:assert/strict';
import test from 'node:test';
import {
  initTheme,
  ToolExecutionComponent,
} from '@earendil-works/pi-coding-agent';
import {
  rgbColor,
  styleText,
  stripTerminalSequences,
  visibleWidth,
} from '@earendil-works/pi-tui';
import {editRenderers, writeRenderers} from './ui-fixture.mjs';
import {SourceView} from '../src/ui/previews/view.ts';

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
const plain = lines => stripTerminalSequences(lines.join('\n'));

test('source previews keep graphemes and reject terminal control sequences', () => {
  const source = new SourceView({
    source: 'const emoji = "👩‍💻 é";\nconst x = "\x1b[2Junsafe";',
    path: 'sample.ts',
    theme,
  });
  const output = source.render(80).join('\n');
  assert.match(stripTerminalSequences(output), /👩‍💻 é/);
  assert.doesNotMatch(output, /\x1b\[2J/);
});

function writeRow(content) {
  const row = new ToolExecutionComponent(
    'write',
    'write-1',
    {path: 'sample.ts', content},
    {outputPad: 1},
    writeRenderers(),
    {requestRender() {}},
    '/missing/project',
  );
  row.setArgsComplete();
  row.updateResult({
    content: [{type: 'text', text: 'Successfully wrote file'}],
    isError: false,
  });
  return row;
}

test('write uses compact summaries and a single-column source preview, not a fabricated diff', () => {
  const row = writeRow('const one = 1;\nconst two = 2;\n');
  assert.equal(row.render(140).filter(line => line.trim()).length, 1);
  assert.match(plain(row.render(140)), /write +sample\.ts +2 lines/);
  row.setExpanded(true);
  const expanded = plain(row.render(140));
  assert.match(expanded, /1  const one/);
  assert.match(expanded, /2  const two/);
  assert.doesNotMatch(expanded, /before|after|@@/);
  row.updateResult({
    content: [{type: 'text', text: 'Permission denied\nCheck access'}],
    isError: true,
  });
  row.setExpanded(false);
  assert.match(plain(row.render(140)), /✗ write[\s\S]*Permission denied/);
  row.setExpanded(true);
  assert.match(plain(row.render(140)), /Check access/);
  assert.doesNotMatch(plain(row.render(140)), /const one/);
});

test('write progress follows streamed arguments, execution, and completion without timers', () => {
  const row = new ToolExecutionComponent(
    'write',
    'stream',
    {path: 'sample.ts', content: 'const'},
    {outputPad: 1},
    writeRenderers(),
    {requestRender() {}},
    '/missing/project',
  );
  assert.match(plain(row.render(120)), /· write +sample\.ts +~5 chars/);
  row.updateArgs({path: 'sample.ts', content: 'x'.repeat(4200)});
  assert.match(plain(row.render(120)), /· write +sample\.ts +~4\.2k chars/);
  row.setArgsComplete();
  assert.match(plain(row.render(120)), /◦ write/);
  row.markExecutionStarted();
  assert.match(plain(row.render(120)), /[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏] write/);
  row.updateResult({
    content: [{type: 'text', text: 'Successfully wrote file'}],
    isError: false,
  });
  assert.match(plain(row.render(120)), /\n [▸▾] write/);
  assert.doesNotMatch(plain(row.render(120)), /receiving|writing…|queued/);
});

test('empty writes, incomplete arguments, and oversized source remain bounded', () => {
  const empty = writeRow('');
  assert.match(plain(empty.render(80)), /0 lines/);
  const partial = new ToolExecutionComponent(
    'write',
    'partial',
    {},
    undefined,
    writeRenderers(),
    {requestRender() {}},
    '/missing',
  );
  partial.setExpanded(true);
  assert.doesNotMatch(plain(partial.render(80)), /written|failed/);
  const huge = writeRow('x'.repeat(500_001));
  huge.setExpanded(true);
  assert.match(plain(huge.render(80)), /display limit/);
});

test('compact summaries color meaning, not paths or write line counts', () => {
  const colors = {
    text: 37,
    toolDiffAdded: 32,
    toolDiffRemoved: 31,
    success: 32,
    error: 31,
    dim: 90,
    muted: 90,
    toolTitle: 37,
  };
  const palette = {
    ...theme,
    fg: (role, text) => `\x1b[${colors[role]}m${text}\x1b[39m`,
  };
  for (const name of ['edit', 'write']) {
    const renderer = name === 'edit' ? editRenderers() : writeRenderers();
    const context = {
      state: {},
      args: {path: 'sample.ts', content: 'one\ntwo\n'},
      argsComplete: true,
      expanded: false,
      isError: false,
      outputPad: 1,
      invalidate() {},
    };
    renderer.renderCall(context.args, palette, context);
    const result = {
      content: [{type: 'text', text: 'Done'}],
      details: {diff: '-1 old\n+1 new'},
    };
    const body = renderer.renderResult(
      result,
      {expanded: false, isPartial: false},
      palette,
      context,
    );
    const output = body.render(120).join('\n');
    assert.ok(output.includes('\x1b[37msample.ts'), JSON.stringify(output));
    if (name === 'edit') {
      assert.ok(output.includes('\x1b[32m+1\x1b[39m'));
      assert.match(output, /\x1b\[31m−1/);
    } else {
      assert.ok(output.includes('\x1b[90m2 lines'), JSON.stringify(output));
      assert.doesNotMatch(plain([output]), /written|✓/);
      assert.match(output, /\x1b\[1m/);
    }
    for (const width of [1, 3, 10, 20, 40])
      for (const line of body.render(width))
        assert.ok(visibleWidth(line) <= width);
    const failed = renderer.renderResult(
      {content: [{type: 'text', text: 'Denied'}]},
      {expanded: false, isPartial: false},
      palette,
      {...context, isError: true},
    );
    assert.match(plain(failed.render(120)), /✗[\s\S]*Denied/);
    assert.doesNotMatch(plain(failed.render(120)), /failed/);
    assert.doesNotMatch(failed.render(120).join('\n'), /\x1b\[48;2;/);
    assert.doesNotMatch(failed.render(120).join('\n'), /\x1b\[32m/);
  }
});
