import assert from 'node:assert/strict';
import test from 'node:test';
import {
  UserMessageComponent,
  getSelectListTheme,
  getMarkdownTheme,
  initTheme,
} from '@earendil-works/pi-coding-agent';
import {
  SelectList,
  Text,
  Markdown,
  HStack,
  VStack,
  stripTerminalSequences,
  visibleWidth,
  getOsc8LinkAtColumn,
  colorToRgb,
  parseColor,
  type Color,
  type TerminalColorMode,
} from '@earendil-works/pi-tui';
import {backgroundCells} from './fixtures/ansi.ts';
import {
  Panel,
  PriorityRow,
  fitLabel,
  readableForeground,
  type PanelModel,
  type RowModel,
} from '../src/ui/index.ts';
// Exact-version host harness; production components receive their host's theme.
import {
  getThemeByName,
  loadThemeFromPath,
} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';
initTheme('dark', false);
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
const theme = getThemeByName('dark');
assert.ok(theme);
const builtin = (name: string, mode: TerminalColorMode = 'truecolor') =>
  loadThemeFromPath(
    new URL(
      `../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/${name}.json`,
      import.meta.url,
    ).pathname,
    mode,
  );
const dark = builtin('dark');
const light = builtin('light');
function luminance(color: Color): number {
  const {r, g, b} = colorToRgb(color);
  const linear = (byte: number) => {
    const s = byte / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}
function contrast(a: Color, b: Color): number {
  const first = luminance(a);
  const second = luminance(b);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}

test('panel framing stays cell-safe, theme-owned and subordinate to content', () => {
  const panel = new Panel();
  const model: PanelModel = {
    heading: {
      layout: 'inline',
      start: [
        {
          text: 'Goal café 日本語 👩🏽‍💻\x1b]52;c;payload\x07',
          tone: 'accent',
          priority: 100,
          shrink: 'clip',
          bold: true,
        },
      ],
      end: [],
    },
    body: new Text('A recorded task\nAnother task', 0, 0),
    footer: {
      start: [
        {text: 'Recorded progress', tone: 'dim', priority: 20, shrink: 'omit'},
      ],
      end: [
        {text: 'inspect · /goal', tone: 'muted', priority: 100, shrink: 'clip'},
      ],
    },
  };
  const rendered: string[][] = [];
  for (const name of ['dark', 'light']) {
    panel.update(model, getThemeByName(name)!);
    for (const width of [0, 1, 2, 8, 11, 12, 13, 20, 40, 80, 120, 240, 400]) {
      const lines = panel.render(width);
      assert.ok(lines.every(line => visibleWidth(line) <= width));
      const clean = lines.map(stripTerminalSequences);
      assert.doesNotMatch(clean.join('\n'), /\x1b|\x07|payload|\uFFFD/);
      if (width >= 12) {
        assert.ok(clean[0]?.startsWith('╭'));
        assert.ok(clean[0]?.endsWith('╮'));
        assert.ok(clean.at(-1)?.startsWith('╰'));
        assert.ok(clean.at(-1)?.endsWith('╯'));
        assert.ok(lines.every(line => visibleWidth(line) === width));
      } else assert.doesNotMatch(clean.join('\n'), /[╭╮╰╯│]/);
    }
    rendered.push(panel.render(80));
  }
  assert.equal(plain(rendered[0]!), plain(rendered[1]!));
  assert.notDeepEqual(rendered[0], rendered[1]);
  assert.match(plain(rendered[0]!), /A recorded task/);
});

test('opaque panel surfaces survive native row and Markdown style resets at every column', () => {
  for (const name of ['light', 'dark'])
    for (const mode of ['truecolor', '256color'] as const) {
      const theme = builtin(name, mode);
      const row = new PriorityRow();
      row.update(
        {
          start: [
            {
              text: 'Task 日本語 👩‍💻',
              tone: 'text',
              priority: 100,
              shrink: 'clip',
            },
          ],
          end: [],
        },
        theme,
      );
      const panel = new Panel();
      panel.update(
        {
          heading: {
            start: [
              {text: 'Goal', tone: 'accent', priority: 100, shrink: 'clip'},
            ],
            end: [],
          },
          body: new VStack([
            new HStack([row, new Text(theme.fg('muted', 'Detail'), 0, 0)]),
            new Markdown(
              '**Result** with [a link](https://example.com) and `code`.',
              0,
              0,
              getMarkdownTheme(),
            ),
          ]),
          footer: {
            start: [
              {text: 'Esc close', tone: 'dim', priority: 100, shrink: 'clip'},
            ],
            end: [],
          },
        },
        theme,
      );
      for (const width of [1, 8, 12, 24, 80])
        for (const [line, rendered] of panel.render(width).entries()) {
          assert.deepEqual(
            backgroundCells(rendered),
            Array<string>(visibleWidth(rendered)).fill(
              theme.getBgAnsi('customMessageBg'),
            ),
            `${name}/${mode}/${width}/row ${line}`,
          );
        }
    }
});

test('framed Pi messages send no prompt marks, which would land mid-line', () => {
  const panel = new Panel();
  panel.update(
    {
      heading: {start: [], end: []},
      body: new UserMessageComponent('Prompt', getMarkdownTheme(), 0),
    },
    dark,
  );
  for (const width of [8, 40]) {
    const lines = panel.render(width);
    assert.match(plain(lines), /Prompt/);
    assert.deepEqual(
      lines.filter(line => line.includes('\x1b]133;')),
      [],
    );
  }
});

test('frame-owned columns do not inherit an unclosed child background or hyperlink', () => {
  for (const theme of [light, dark]) {
    const child = '\x1b[48;2;10;20;30m';
    const panel = new Panel();
    panel.update(
      {
        heading: {start: [], end: []},
        body: new Text(child + '\x1b]8;;https://example.com\x07X', 0, 0),
      },
      theme,
    );
    const line = panel.render(40)[1]!;
    assert.deepEqual(backgroundCells(line), [
      theme.getBgAnsi('customMessageBg'),
      theme.getBgAnsi('customMessageBg'),
      ...Array<string>(36).fill(child),
      theme.getBgAnsi('customMessageBg'),
      theme.getBgAnsi('customMessageBg'),
    ]);
    assert.equal(getOsc8LinkAtColumn(line, 2), 'https://example.com');
    assert.equal(getOsc8LinkAtColumn(line, 39), undefined);
  }
});

test('native selections retain their background through panel resize and theme changes', () => {
  let theme = light;
  const list = new SelectList(
    [
      {label: 'first', value: 'a'},
      {label: 'second', value: 'b'},
    ],
    8,
    {
      ...getSelectListTheme(),
      selectedText: text => theme.bg('selectedBg', theme.fg('text', text)),
    },
  );
  const panel = new Panel();
  for (const nextTheme of [light, dark, light]) {
    theme = nextTheme;
    panel.update({heading: {start: [], end: []}, body: list}, theme);
    for (const index of [0, 1]) {
      list.setSelectedIndex(index);
      for (const width of [24, 80, 40]) {
        const lines = panel.render(width);
        const selected = lines[index + 1]!;
        const other = lines[2 - index]!;
        const label = index ? 'second' : 'first';
        const at = stripTerminalSequences(selected).indexOf(label);
        assert.deepEqual(
          backgroundCells(selected).slice(at, at + label.length),
          Array<string>(label.length).fill(theme.getBgAnsi('selectedBg')),
        );
        assert.deepEqual(
          backgroundCells(other),
          Array<string>(width).fill(theme.getBgAnsi('customMessageBg')),
        );
      }
    }
  }
});

test('panel forwards native picker mouse coordinates without owning selection', () => {
  const list = new SelectList(
    [
      {label: 'first', value: 'a'},
      {label: 'second', value: 'b'},
    ],
    8,
    getSelectListTheme(),
  );
  let selected = '';
  list.onSelect = item => {
    selected = item.value;
  };
  const panel = new Panel();
  panel.update(
    {
      heading: {
        start: [
          {text: 'Workers', tone: 'accent', priority: 100, shrink: 'clip'},
        ],
        end: [],
      },
      body: list,
    },
    getThemeByName('dark')!,
  );
  panel.render(40);
  const event = {
    button: 'left' as const,
    x: 3,
    y: 2,
    screenX: 3,
    screenY: 2,
    width: 40,
    height: 4,
    shift: false,
    ctrl: false,
    alt: false,
  };
  assert.equal(panel.handleMouse({...event, type: 'press'})?.handled, true);
  assert.equal(panel.handleMouse({...event, type: 'click'})?.handled, true);
  assert.equal(selected, 'b');
  selected = '';
  for (const x of [0, 1, 38, 39]) {
    assert.equal(
      panel.handleMouse({...event, x, screenX: x, type: 'click'}),
      undefined,
    );
  }
  assert.equal(selected, '', 'frame borders cannot activate native choices');
  assert.equal(
    panel.handleMouse({...event, type: 'wheel', wheelDelta: -1})?.handled,
    true,
  );
  assert.equal(
    list.getSelectedItem()?.value,
    'a',
    'body wheel still reaches native selection',
  );
});

test('middle clipping preserves filename tails and grapheme-safe width bounds', () => {
  const path = 'packages/日本語/👩🏽‍💻/a-long-directory/report.ts';
  assert.match(fitLabel(path, 24, 'middle'), /report\.ts$/);
  for (let width = 0; width < 100; width++) {
    const result = fitLabel(path, width, 'middle');
    assert.ok(visibleWidth(result) <= width, `${width}: ${result}`);
    assert.doesNotMatch(result, /\uFFFD/);
  }
});

test('inline and edge layouts differ only in placement, not shrink decisions', () => {
  const model: RowModel = {
    start: [
      {
        text: 'src/日本語/report.ts',
        tone: 'toolTitle',
        priority: 40,
        shrink: 'clip',
        minWidth: 10,
        clip: 'middle',
      },
    ],
    end: [
      {
        text: 'failed',
        tone: 'error',
        priority: 100,
        shrink: 'clip',
        minWidth: 6,
      },
      {text: '12.3s', tone: 'dim', priority: 0, shrink: 'omit'},
    ],
    endSeparator: ' · ',
  };
  const inline = new PriorityRow();
  const edges = new PriorityRow();
  const implicit = new PriorityRow();
  inline.update({...model, layout: 'inline'}, theme);
  edges.update({...model, layout: 'edges'}, theme);
  implicit.update(model, theme);
  for (const width of [...Array.from({length: 100}, (_, n) => n), 240]) {
    const left = plain(inline.render(width));
    const right = plain(edges.render(width));
    assert.ok(visibleWidth(left) <= width);
    assert.ok(visibleWidth(right) <= width);
    assert.equal(
      left.trim().replace(/\s+/g, ' '),
      right.trim().replace(/\s+/g, ' '),
    );
    assert.equal(plain(implicit.render(width)), right);
    if (width >= 6) assert.match(left, /failed/);
    if (width <= 10) assert.doesNotMatch(left, /·/);
  }
  assert.equal(
    plain(inline.render(240)).trimEnd(),
    'src/日本語/report.ts failed · 12.3s',
  );
  assert.equal(visibleWidth(plain(edges.render(240)).trimEnd()), 240);
});

test('both layouts handle missing groups without orphan separators', () => {
  const item = {
    text: 'value',
    tone: 'dim',
    priority: 1,
    shrink: 'omit',
  } as const;
  for (const layout of ['inline', 'edges'] as const) {
    const row = new PriorityRow();
    for (const [start, end] of [
      [[], []],
      [[item], []],
      [[], [item]],
    ] as const) {
      row.update(
        {layout, start, end, separator: ' · ', endSeparator: ' / '},
        theme,
      );
      for (const width of [1, 5, 40]) {
        const rendered = plain(row.render(width));
        assert.doesNotMatch(rendered, /[·/]/);
        assert.ok(visibleWidth(rendered) <= width);
        assert.equal(
          rendered.trim(),
          width >= 5 && (start.length || end.length) ? 'value' : '',
        );
        if (layout === 'inline' && rendered.trim())
          assert.ok(rendered.startsWith('value'));
      }
    }
  }
});

test('group separators belong to surviving groups in both layouts', () => {
  for (const layout of ['inline', 'edges'] as const) {
    const row = new PriorityRow();
    const start = [
      {text: 'read', tone: 'muted', priority: 10, shrink: 'omit'},
    ] as const;
    const end = [
      {
        text: 'done',
        tone: 'success',
        priority: 100,
        shrink: 'clip',
        minWidth: 4,
      },
    ] as const;
    row.update({layout, start, end, groupSeparator: ' · '}, theme);
    assert.match(plain(row.render(60)), /^read · +done/);
    for (let width = 1; width <= 10; width++) {
      const rendered = plain(row.render(width));
      assert.doesNotMatch(rendered, /·/);
      assert.ok(visibleWidth(rendered) <= width);
    }
    for (const groups of [
      {start: [], end},
      {start, end: []},
      {start: [], end: []},
    ]) {
      row.update({layout, ...groups, groupSeparator: ' · '}, theme);
      assert.doesNotMatch(plain(row.render(60)), /·/);
    }
  }
});

test('soft slots align short labels, never cap long labels, and yield before content', () => {
  const row = new PriorityRow();
  for (const layout of ['inline', 'edges'] as const) {
    for (const label of ['$', 'read', 'write', 'codemode', '日本語']) {
      const start = [
        {text: label, tone: 'muted', priority: 90, shrink: 'clip', padTo: 5},
        {
          text: 'done',
          tone: 'success',
          priority: 100,
          shrink: 'clip',
          minWidth: 4,
        },
      ] as const;
      row.update({layout, start, end: [], separator: ' · '}, theme);
      assert.equal(
        plain(row.render(60)).trimEnd(),
        label + ' '.repeat(Math.max(0, 5 - visibleWidth(label))) + ' · done',
      );
      const tight = visibleWidth(label) + 3 + 4;
      assert.equal(plain(row.render(tight)).trim(), `${label} · done`);
    }
  }
  row.update(
    {
      layout: 'inline',
      start: [{text: '', tone: 'dim', priority: 0, shrink: 'clip', padTo: 50}],
      end: [],
    },
    theme,
  );
  assert.equal(plain(row.render(60)).trim(), '');
});

test('segment separators express related metadata without orphan punctuation', () => {
  const row = new PriorityRow();
  for (const layout of ['inline', 'edges'] as const) {
    row.update(
      {
        layout,
        separator: ' · ',
        start: [
          {text: '+3', tone: 'toolDiffAdded', priority: 10, shrink: 'omit'},
          {
            text: '−2',
            tone: 'toolDiffRemoved',
            priority: 100,
            shrink: 'clip',
            separatorBefore: ' ',
          },
        ],
        end: [],
      },
      theme,
    );
    assert.equal(plain(row.render(40)).trim(), '+3 −2');
    assert.equal(plain(row.render(2)).trim(), '−2');
  }
});

test('preferred cell widths align sanitized labels and still yield by priority', () => {
  const row = new PriorityRow();
  for (const layout of ['inline', 'edges'] as const) {
    for (const label of [
      'web',
      'example-api',
      'a-much-longer-label',
      '日本語',
      ' web\t ',
    ]) {
      row.update(
        {
          layout,
          start: [
            {
              text: '?',
              tone: 'warning',
              priority: 100,
              shrink: 'clip',
              minWidth: 1,
            },
            {
              text: label,
              tone: 'toolTitle',
              priority: 90,
              shrink: 'clip',
              width: 12,
              padTo: 30,
            },
            {text: 'asking', tone: 'dim', priority: 50, shrink: 'clip'},
          ],
          end: [],
        },
        theme,
      );
      const wide = plain(row.render(80));
      assert.equal(visibleWidth(wide.slice(0, wide.indexOf('asking'))), 15);
      assert.ok(visibleWidth(wide) <= 80);
      const narrow = plain(row.render(10));
      assert.match(narrow, /^\?/);
      assert.doesNotMatch(narrow, /asking/);
      assert.ok(visibleWidth(narrow) <= 10);
    }
    row.update(
      {
        layout,
        start: [
          {
            text: 'a long subject',
            tone: 'muted',
            priority: 10,
            shrink: 'clip',
            width: 12,
            minWidth: 8,
          },
        ],
        end: [
          {
            text: 'failed',
            tone: 'error',
            priority: 100,
            shrink: 'clip',
            minWidth: 6,
          },
        ],
      },
      theme,
    );
    assert.equal(plain(row.render(6)).trim(), 'failed');
  }
});

test('row indentation is layout-owned and stays bounded in narrow panes', () => {
  const row = new PriorityRow();
  for (const layout of ['inline', 'edges'] as const) {
    for (const indent of [0, 4]) {
      row.update(
        {
          layout,
          indent,
          start: [
            {
              text: '  ⤷',
              tone: 'muted',
              priority: 100,
              shrink: 'clip',
              minWidth: 1,
            },
            {text: 'twig', tone: 'toolTitle', priority: 50, shrink: 'clip'},
          ],
          end: [],
        },
        theme,
      );
      assert.equal(plain(row.render(40)).indexOf('⤷'), indent);
      assert.deepEqual(row.render(0), []);
      for (const width of [1, 2, 4, 5, 8, 40]) {
        const lines = row.render(width);
        assert.equal(lines.length, 1);
        assert.ok(visibleWidth(plain(lines)) <= width);
        assert.equal(plain(lines).indexOf('⤷'), Math.min(indent, width - 1));
      }
    }
  }
});

test('bold identity and failure badges retain text and never overflow', () => {
  const row = new PriorityRow();
  for (const appearance of ['light', 'dark']) {
    const palette = getThemeByName(appearance);
    assert.ok(palette);
    row.update(
      {
        layout: 'inline',
        start: [
          {
            text: 'bash',
            tone: 'toolTitle',
            bold: true,
            priority: 90,
            shrink: 'clip',
          },
          {
            text: 'failed',
            tone: 'error',
            badge: 'error',
            minWidth: 8,
            priority: 100,
            shrink: 'clip',
          },
        ],
        end: [],
      },
      palette,
    );
    assert.match(plain(row.render(80)), /bash +failed/);
    assert.match(row.render(80).join(''), /\x1b\[1m/);
    assert.match(row.render(80).join(''), /\x1b\[48;/);
    for (let width = 0; width < 80; width++)
      assert.ok(row.render(width).every(line => visibleWidth(line) <= width));
  }
});

test('row shrink priority is independent of whether a segment declares a floor', () => {
  const row = new PriorityRow();
  for (const minWidth of [undefined, 8]) {
    row.update(
      {
        start: [
          {text: '?', tone: 'warning', priority: 100, shrink: 'clip'},
          {
            text: 'example-web',
            tone: 'toolTitle',
            priority: 90,
            shrink: 'clip',
            ...(minWidth === undefined ? {} : {minWidth}),
          },
          {
            text: 'asking: Which ingress?',
            tone: 'dim',
            priority: 50,
            shrink: 'clip',
          },
        ],
        end: [],
      },
      theme,
    );
    const rendered = plain(row.render(30));
    assert.match(rendered, /\? example-web asking:/);
    assert.doesNotMatch(rendered, /Which ingress\?/);
    assert.ok(visibleWidth(rendered) <= 30);
  }
});

test('explicit floors protect pass one and yield by priority in pass two', () => {
  const row = new PriorityRow();
  row.update(
    {
      start: [
        {
          text: 'abcdefghij',
          tone: 'muted',
          priority: 10,
          shrink: 'clip',
          minWidth: 4,
        },
        {text: '0123456789', tone: 'toolTitle', priority: 90, shrink: 'clip'},
      ],
      end: [],
    },
    theme,
  );
  const first = plain(row.render(10));
  const [low, high] = first.trim().split(' ');
  assert.ok(low && high);
  assert.equal(visibleWidth(low), 4); // Four-cell floor survives the first pass.
  assert.equal(visibleWidth(high), 5);
  assert.match(low, /^a/);
  assert.match(high, /^0/);
  assert.ok(visibleWidth(first) <= 10);
  row.update(
    {
      start: [
        {
          text: 'abcdefghij',
          tone: 'muted',
          priority: 10,
          shrink: 'clip',
          minWidth: 4,
        },
        {
          text: 'failed',
          tone: 'error',
          priority: 100,
          shrink: 'clip',
          minWidth: 6,
        },
      ],
      end: [],
    },
    theme,
  );
  assert.equal(plain(row.render(6)).trim(), 'failed');
});

test('layout owns optional separators and preserves outcomes ahead of desired subject widths', () => {
  const row = new PriorityRow();
  row.update(
    {
      start: [
        {
          text: 'very-long-subject.ts',
          tone: 'toolTitle',
          priority: 40,
          shrink: 'clip',
          minWidth: 16,
          clip: 'middle',
        },
      ],
      end: [
        {text: 'low-priority', tone: 'dim', priority: 0, shrink: 'omit'},
        {
          text: 'failed',
          tone: 'error',
          priority: 100,
          shrink: 'clip',
          minWidth: 6,
        },
      ],
      endSeparator: ' · ',
    },
    theme,
  );
  for (const width of [8, 16, 25]) {
    const text = plain(row.render(width));
    assert.match(text, /failed/);
    assert.doesNotMatch(text, /·/);
    assert.ok(visibleWidth(text) <= width);
  }
});

test('row priority is independent of tool vocabulary and responsive to new themes', () => {
  const row = new PriorityRow();
  const model = {
    start: [
      {text: '漢字 workspace', tone: 'muted', priority: 1, shrink: 'clip'},
    ],
    end: [
      {text: 'optional branch', tone: 'dim', priority: 0, shrink: 'omit'},
      {text: 'ctx 95%', tone: 'error', priority: 100, shrink: 'clip'},
    ],
  } as const;
  row.update(model, dark);
  const before = row.render(80);
  row.update(model, light);
  const after = row.render(80);
  assert.equal(plain(before), plain(after));
  assert.notDeepEqual(before, after);
  assert.match(plain(row.render(10)), /ctx 95%/);
  assert.doesNotMatch(plain(row.render(10)), /optional/);
  for (const width of [1, 2, 3, 5, 20, 80])
    for (const line of row.render(width))
      assert.ok(visibleWidth(line) <= width);
});

test('contrast adjustment preserves already-readable colors and moves both light and dark foregrounds', () => {
  for (const [foreground, background] of [
    ['#eeeeee', '#111111'],
    ['#777777', '#dddddd'],
    ['#555555', '#222222'],
  ]) {
    assert.ok(foreground && background);
    const fg = parseColor(foreground);
    const bg = parseColor(background);
    const result = readableForeground(fg, bg);
    assert.ok(contrast(result, bg) >= 4.5);
    if (contrast(fg, bg) >= 4.5) assert.deepEqual(result, fg);
    assert.deepEqual(readableForeground(fg, bg), result);
  }
});
