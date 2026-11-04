import assert from 'node:assert/strict';
import test from 'node:test';
import {
  getOsc8LinkAtColumn,
  stripTerminalSequences,
  visibleWidth,
} from '@earendil-works/pi-tui';
import {paintSurfaceLine} from '../src/ui/index.ts';
import {backgroundCells} from './fixtures/ansi.ts';

const colors = {fg: '\x1b[38;2;17;18;19m', bg: '\x1b[48;2;21;22;23m'};

test('surface defaults survive full, empty, individual and compound SGR resets', () => {
  for (const sequence of [
    '\x1b[0m',
    '\x1b[m',
    '\x1b[49m',
    '\x1b[39m',
    '\x1b[22;49m',
    '\x1b[39;49m',
    '\x1b[31;0m',
  ]) {
    const line = paintSurfaceLine(`A${sequence}B`, 4, colors);
    assert.deepEqual(
      backgroundCells(line),
      Array<string>(4).fill(colors.bg),
      sequence,
    );
    assert.equal(stripTerminalSequences(line), 'AB  ');
    assert.ok(line.includes(sequence), 'child attributes are not stripped');
  }
});

test('explicit child colors win in parameter order, without treating color payloads as resets', () => {
  for (const {sequence, background} of [
    {sequence: '\x1b[41m', background: '\x1b[41m'},
    {sequence: '\x1b[104m', background: '\x1b[104m'},
    {sequence: '\x1b[48;5;0m', background: '\x1b[48;5;0m'},
    {sequence: '\x1b[48;5;49m', background: '\x1b[48;5;49m'},
    {sequence: '\x1b[48;2;0;39;49m', background: '\x1b[48;2;0;39;49m'},
    {sequence: '\x1b[0;48;2;10;20;30m', background: '\x1b[48;2;10;20;30m'},
    {sequence: '\x1b[49;48;5;39m', background: '\x1b[48;5;39m'},
    {sequence: '\x1b[48;5;39;49m', background: colors.bg},
    {sequence: '\x1b[48;5;39;0m', background: colors.bg},
    {sequence: '\x1b[38;2;0;39;49m', background: colors.bg},
  ]) {
    const line = paintSurfaceLine(`A${sequence}B\x1b[49mC`, 4, colors);
    assert.deepEqual(
      backgroundCells(line),
      [colors.bg, background, colors.bg, colors.bg],
      sequence,
    );
  }
  for (const sequence of [
    '\x1b[48:2::0:39:49m',
    '\x1b[48:5:49m',
    '\x1b[0;48:2::0:39:49m',
    '\x1b[58;2;0;39;49m',
  ]) {
    // Pi's test oracle does not track colon-form or underline colors. Verify
    // byte preservation without treating their payloads as reset instructions.
    const line = paintSurfaceLine(`${sequence}X`, 1, colors);
    const restoredForeground = sequence.startsWith('\x1b[0;') ? colors.fg : '';
    assert.ok(line.includes(sequence + restoredForeground + 'X'), sequence);
  }
});

test('surface padding cannot inherit unclosed child colors, inverse attributes or links', () => {
  const child = '\x1b[48;2;10;20;30m';
  const line = paintSurfaceLine(`${child}X`, 4, colors);
  assert.deepEqual(backgroundCells(line), [
    child,
    colors.bg,
    colors.bg,
    colors.bg,
  ]);
  const inverse = paintSurfaceLine('\x1b[7mX', 3, colors);
  assert.ok(inverse.includes('\x1b[7mX\x1b[0m'));
  const link = paintSurfaceLine('\x1b]8;;https://example.com\x07X', 3, colors);
  assert.equal(getOsc8LinkAtColumn(link, 0), 'https://example.com');
  assert.equal(getOsc8LinkAtColumn(link, 1), undefined);
  assert.equal(getOsc8LinkAtColumn(link, 2), undefined);
  assert.equal(
    backgroundCells(line + 'outside').at(-1),
    '',
    'surface colors do not escape to following content',
  );
  assert.equal(getOsc8LinkAtColumn(link + 'outside', 3), undefined);
});

test('nested surfaces preserve inner colors and return to the outer surface', () => {
  const innerColors = {fg: '\x1b[32m', bg: '\x1b[44m'};
  const inner = paintSurfaceLine('内', 4, innerColors);
  const outer = paintSurfaceLine(inner + 'outer', 10, colors);
  assert.deepEqual(backgroundCells(outer), [
    ...Array<string>(4).fill(innerColors.bg),
    ...Array<string>(6).fill(colors.bg),
  ]);
  assert.equal(stripTerminalSequences(outer), '内  outer ');
});

test('opaque terminal control strings are not mistaken for SGR instructions', () => {
  for (const opaque of [
    '\x1b]8;;https://example.com/\x1b[49m\x07',
    '\x1b]8;;https://example.com/\x1b[0m\x1b\\',
    '\x1bPpayload\x1b[0m\x1b\\',
    '\x1b_Gpayload\x1b[49m\x1b\\',
    '\x1b^payload\x1b[39m\x1b\\',
  ]) {
    const line = paintSurfaceLine(
      opaque + 'X',
      visibleWidth(opaque + 'X'),
      colors,
    );
    assert.ok(line.includes(opaque + 'X'), JSON.stringify(opaque));
  }
});

test('surface lines stay bounded, retain Unicode and links, and support terminal-default colors', () => {
  const content = '\x1b]8;;https://example.com\x07日本語 👩‍💻 café\x1b]8;;\x07';
  for (const width of [0, 1, 2, 3, 8, 20, 80]) {
    const line = paintSurfaceLine(content, width, colors);
    assert.equal(visibleWidth(line), width);
    assert.doesNotMatch(stripTerminalSequences(line), /\uFFFD/);
    assert.deepEqual(
      backgroundCells(line),
      Array<string>(width).fill(colors.bg),
    );
    if (width >= 2)
      assert.equal(getOsc8LinkAtColumn(line, 0), 'https://example.com');
  }
  const defaults = paintSurfaceLine('A\x1b[0mB', 4, {
    fg: '\x1b[39m',
    bg: '\x1b[49m',
  });
  assert.deepEqual(backgroundCells(defaults), ['', '', '', '']);
  assert.doesNotMatch(defaults, /\x1b\[(?:38|48);/);
});
