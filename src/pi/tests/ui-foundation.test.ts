import assert from 'node:assert/strict';
import {test} from 'node:test';
import {
  createWriteToolDefinition,
  defineTool,
  initTheme,
  ToolExecutionComponent,
  type Theme,
} from '@earendil-works/pi-coding-agent';
import {
  ProcessTerminal,
  TuiMainScreen,
  Text,
  stripTerminalSequences,
  visibleWidth,
} from '@earendil-works/pi-tui';
import {ActivityCard, activityHeader, OutputView} from '../src/ui/card.ts';
import {
  createToolRenderers,
  type PresentationView,
  type ToolObservation,
  type ToolPresentation,
} from '../src/host/tools.ts';
import {DetailView} from '../src/ui/details.ts';
import {SourceView, DiffView} from '../src/ui/previews/view.ts';
// Version-pinned host harness only; production imports use public package roots.
import {loadThemeFromPath} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';

initTheme('dark', false);
// Built-in themes at a fixed color depth, so rendered escapes do not depend on
// the COLORTERM of whoever runs the tests.
const builtin = (name: string) =>
  loadThemeFromPath(
    new URL(
      `../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/${name}.json`,
      import.meta.url,
    ).pathname,
    'truecolor',
  );
const dark = builtin('dark');
const light = builtin('light');
assert.ok(dark && light);
const tui = new TuiMainScreen(new ProcessTerminal()); // Not started; no terminal input/output ownership.
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));

// This independent consumer imports no builtin tool feature or UI registration.
interface ExampleModel {
  readonly text: string;
}
class ExampleView implements PresentationView<ExampleModel> {
  private text = new Text('', 0, 0);
  update(model: ExampleModel, theme: Theme): void {
    this.text.setText(theme.fg('toolOutput', model.text));
  }
  invalidate(): void {
    this.text.invalidate();
  }
  render(width: number): string[] {
    return this.text.render(width);
  }
}
const example = {
  shell: 'self',
  project(frame: ToolObservation): ExampleModel {
    if (frame.slot === 'call') return {text: 'example invocation'};
    // An unknown result remains in this consumer's view; no renderer handoff.
    return {
      text:
        frame.content
          .filter(part => part.type === 'text')
          .map(part => part.text)
          .join('\n') || 'Recorded non-text result',
    };
  },
  create: () => new ExampleView(),
} satisfies ToolPresentation<ExampleModel>;

test('typed custom tool can use descriptor-to-renderers without registering our extension', () => {
  const original = createWriteToolDefinition('/missing/project');
  const custom = defineTool({
    ...original,
    name: 'example',
    ...createToolRenderers(example),
  });
  assert.equal(custom.execute, original.execute);
  assert.equal(custom.parameters, original.parameters);
  const component = new ToolExecutionComponent(
    'example',
    'one',
    {},
    {outputPad: 1},
    custom,
    tui,
    '/missing/project',
  );
  assert.match(plain(component.render(80)), /example invocation/);
  component.updateResult({
    content: [{type: 'text', text: 'Recorded fallback'}],
    details: {unsupported: true},
    isError: false,
  });
  assert.match(plain(component.render(80)), /Recorded fallback/);
  assert.doesNotMatch(plain(component.render(80)), /example invocation/);
});

test('default shell presentations render inside the host box', () => {
  const nativeShell = createToolRenderers({...example, shell: 'default'});
  const component = new ToolExecutionComponent(
    'example',
    'native',
    {},
    undefined,
    nativeShell,
    tui,
    '/missing/project',
  );
  component.updateResult({
    content: [{type: 'text', text: 'default shell content'}],
    isError: true,
  });
  assert.match(plain(component.render(80)), /default shell content/);
});

test('source and diff previews use Pi styling and obey budgets even at one column', () => {
  const source = new SourceView({
    source: 'line\n'.repeat(40_000),
    path: 'example.txt',
    theme: dark,
  });
  assert.match(plain(source.render(80)), /display limited/);
  const code = new SourceView({
    source: 'const a = 1;',
    path: 'a.ts',
    theme: dark,
  });
  const highlighted = code.render(80);
  assert.equal(code.render(80), highlighted, 'rows are reused per width');
  assert.deepEqual(highlighted, code.render(80));
  assert.match(plain(highlighted), /1  const a = 1;/);
  const streaming = new SourceView({
    source: 'const a = 1;',
    path: 'a.ts',
    theme: dark,
    highlight: false,
  });
  assert.notDeepEqual(streaming.render(80), highlighted);
  const diff = new DiffView(' 1 keep\n-2 old value\n+2 new value', dark);
  assert.match(plain(diff.render(80)), /-2 old value\n\+2 new value/);
  const large = new DiffView('+1 x\n'.repeat(20_000), dark);
  assert.match(plain(large.render(80)), /display limited/);
  for (const width of [1, 2, 20])
    for (const view of [source, diff])
      assert.ok(view.render(width).every(line => visibleWidth(line) <= width));
  const output = new OutputView('head\n'.repeat(4_000) + 'EXIT_7');
  for (const width of [1, 2, 20, 100]) {
    const lines = output.render(width);
    assert.ok(lines.length <= 2_000);
    assert.ok(lines.every(line => visibleWidth(line) <= width));
    assert.ok(plain(lines).replaceAll('\n', '').endsWith('EXIT_7'));
  }
});

test('detail frames reuse framed rows until content, width or styling changes', () => {
  let rows = ['first', 'second'];
  const child = {render: () => rows, invalidate() {}};
  const detail = new DetailView(child, 'caption', dark);
  const framed = detail.render(40);
  assert.equal(detail.render(40), framed);
  assert.notEqual(detail.render(30), framed);
  rows = ['first', 'third'];
  assert.match(plain(detail.render(40)), /third/);
  const settled = detail.render(40);
  detail.invalidate();
  const restyled = detail.render(40);
  assert.notEqual(restyled, settled);
  assert.deepEqual(restyled, settled);
});

test('a card keeps its detail frame across body-less updates and still invalidates it', () => {
  let invalidated = 0;
  const body = {
    render: () => ['recorded'],
    invalidate() {
      invalidated++;
    },
  };
  const card = new ActivityCard();
  const model = {
    header: activityHeader('write', 'a.ts', []),
    notices: [],
    caption: '',
  };
  card.update({...model, body}, dark);
  card.update({...model, body: undefined}, dark);
  card.invalidate();
  assert.equal(invalidated, 1);
  card.update({...model, body}, dark);
  assert.match(plain(card.render(40)), /recorded/);
});
