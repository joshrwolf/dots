import assert from 'node:assert/strict';
import {after, test} from 'node:test';
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
import {
  ActivityCard,
  activityHeader,
  createSyntaxService,
  createToolRenderers,
  DetailView,
  OutputView,
  selectToolRenderers,
  SourceView,
  DiffView,
  parsePatch,
  type PresentationView,
  type SyntaxService,
  type Tokens,
  type ToolObservation,
  type ToolPresentation,
} from '@joshrwolf/pi-dots/ui';
import type {SyntaxEngine} from '../src/ui/previews/syntax.ts';
import {parsePreferences} from '../src/host/preferences.ts';
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

function deferred<T>() {
  let resolve: ((value: T) => void) | undefined;
  const promise = new Promise<T>(done => {
    resolve = done;
  });
  return {
    promise,
    resolve(value: T) {
      assert.ok(resolve);
      resolve(value);
    },
  };
}

const services: SyntaxService[] = [];
after(() => {
  for (const service of services) service.dispose();
});

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

test('resolver delegation is only by selection, with explicit stable shell ownership', () => {
  const selected = createToolRenderers(example);
  let delegates = 0;
  const next = () => {
    delegates++;
    return {renderShell: 'default' as const};
  };
  const resolve = selectToolRenderers(new Map([['example', selected]]));
  assert.equal(resolve('example', next), selected);
  assert.equal(delegates, 0);
  assert.equal(resolve('foreign', next)?.renderShell, 'default');
  assert.equal(delegates, 1);
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

test('syntax service is lazy and usable before session_start, deduplicating engine and grammar', async () => {
  let engines = 0;
  let loads = 0;
  let disposed = 0;
  const syntax = createSyntaxService(async () => {
    engines++;
    return {
      async load() {
        loads++;
      },
      tokenize: source => [[{content: source, offset: 0}]],
      dispose() {
        disposed++;
      },
    };
  });
  assert.equal(engines, 0);
  await Promise.all([
    syntax.highlight('const a = 1', 'a.ts', 'dark'),
    syntax.highlight('const b = 2', 'b.ts', 'light'),
  ]);
  assert.equal(engines, 1);
  assert.equal(loads, 1);
  syntax.dispose();
  syntax.dispose();
  await new Promise<void>(resolve => setImmediate(resolve));
  assert.equal(disposed, 1);
  await syntax.highlight('later', 'a.ts', 'dark');
  assert.equal(engines, 1);
});

test('shutdown while engine initializes disposes the late engine exactly once', async () => {
  const init = deferred<SyntaxEngine>();
  let disposed = 0;
  let tokenized = 0;
  const syntax = createSyntaxService(() => init.promise);
  let redraws = 0;
  const source = new SourceView({
    source: 'const value = 1',
    path: 'a.ts',
    theme: dark,
    syntax,
    invalidate: () => redraws++,
  });
  const pending = source.prepare();
  syntax.dispose();
  syntax.dispose();
  init.resolve({
    async load() {},
    tokenize() {
      tokenized++;
      return [];
    },
    dispose() {
      disposed++;
    },
  });
  await pending;
  await new Promise<void>(resolve => setImmediate(resolve));
  assert.equal(disposed, 1);
  assert.equal(tokenized, 0);
  assert.equal(redraws, 0);
  assert.match(plain(source.render(80)), /const value/);
});

test('shutdown waits for an in-flight grammar before disposing its engine', async () => {
  const loading = deferred<void>();
  const started = deferred<void>();
  let disposed = 0;
  const syntax = createSyntaxService(async () => ({
    load() {
      started.resolve(undefined);
      return loading.promise;
    },
    tokenize: () => [],
    dispose() {
      disposed++;
    },
  }));
  const work = syntax.highlight('retained', 'a.ts', 'dark');
  await started.promise;
  syntax.dispose();
  assert.equal(disposed, 0);
  loading.resolve(undefined);
  const result = await work;
  await new Promise<void>(resolve => setImmediate(resolve));
  assert.equal(result[0]?.[0]?.content, 'retained');
  assert.equal(disposed, 1);
});

test('dark-light-dark completions cannot commit a stale same-appearance request', async () => {
  const requests: ReturnType<typeof deferred<Tokens>>[] = [];
  const syntax: SyntaxService = {
    active: true,
    failure: undefined,
    highlight() {
      const request = deferred<Tokens>();
      requests.push(request);
      return request.promise;
    },
    dispose() {},
  };
  let redraws = 0;
  const view = new SourceView({
    source: 'value',
    path: 'a.ts',
    theme: dark,
    syntax,
    invalidate: () => redraws++,
  });
  const first = view.prepare();
  view.setTheme(light);
  const second = view.prepare();
  view.setTheme(dark);
  const third = view.prepare();
  assert.equal(requests.length, 3);
  requests[2]?.resolve([[{content: 'value', offset: 0, color: '#00ff00'}]]);
  await third;
  requests[0]?.resolve([[{content: 'value', offset: 0, color: '#ff0000'}]]);
  requests[1]?.resolve([[{content: 'value', offset: 0, color: '#0000ff'}]]);
  await Promise.all([first, second]);
  assert.equal(redraws, 1);
  assert.match(view.render(80).join('\n'), /38;2;0;255;0/);
});

test('diff appearance races cannot commit stale tokens', async () => {
  const requests: ReturnType<typeof deferred<Tokens>>[] = [];
  const syntax: SyntaxService = {
    active: true,
    failure: undefined,
    dispose() {},
    highlight() {
      const request = deferred<Tokens>();
      requests.push(request);
      return request.promise;
    },
  };
  let redraws = 0;
  const patch = parsePatch('@@ -1 +1 @@\n-old\n+new');
  assert.ok(patch);
  const view = new DiffView({
    patch,
    path: 'a.ts',
    theme: dark,
    syntax,
    invalidate: () => redraws++,
  });
  const first = view.prepare();
  view.setTheme(light);
  const second = view.prepare();
  view.setTheme(dark);
  const third = view.prepare();
  assert.equal(requests.length, 6);
  for (const request of requests.slice(4))
    request.resolve([[{content: 'new', offset: 0, color: '#00ff00'}]]);
  await third;
  const current = view.render(80);
  for (const request of requests.slice(0, 4))
    request.resolve([[{content: 'old', offset: 0, color: '#ff0000'}]]);
  await Promise.all([first, second]);
  assert.equal(redraws, 1);
  assert.deepEqual(view.render(80), current);
});

test('replacing a source view cannot recolor or replace its new document', async () => {
  const requests: ReturnType<typeof deferred<Tokens>>[] = [];
  const syntax: SyntaxService = {
    active: true,
    failure: undefined,
    dispose() {},
    highlight() {
      const request = deferred<Tokens>();
      requests.push(request);
      return request.promise;
    },
  };
  let current = new SourceView({
    source: 'old',
    path: 'a.ts',
    theme: dark,
    syntax,
  });
  const oldWork = current.prepare();
  current = new SourceView({source: 'new', path: 'a.ts', theme: dark, syntax});
  const newWork = current.prepare();
  requests[1]?.resolve([[{content: 'new', offset: 0, color: '#00ff00'}]]);
  await newWork;
  const before = current.render(80);
  requests[0]?.resolve([[{content: 'old', offset: 0, color: '#ff0000'}]]);
  await oldWork;
  assert.deepEqual(current.render(80), before);
  assert.match(plain(current.render(80)), /new/);
});

test('syntax failures are observable while recorded source stays readable', async () => {
  const problem = new Error('fixture grammar failure');
  const syntax = createSyntaxService(async () => {
    throw problem;
  });
  services.push(syntax);
  const tokens = await syntax.highlight('retained', 'a.ts', 'dark');
  assert.equal(tokens[0]?.[0]?.content, 'retained');
  assert.equal(syntax.failure, problem);
});

test('source preparation and output tails obey budgets even at one column', async () => {
  let prepared = '';
  const syntax: SyntaxService = {
    active: true,
    failure: undefined,
    dispose() {},
    async highlight(source) {
      prepared = source;
      return [];
    },
  };
  const source = new SourceView({
    source: 'line\n'.repeat(40_000),
    path: 'example.txt',
    theme: dark,
    syntax,
  });
  await source.prepare();
  assert.ok(prepared.split('\n').length <= 10_001);
  assert.match(plain(source.render(80)), /display limited/);
  const output = new OutputView('head\n'.repeat(4_000) + 'EXIT_7');
  for (const width of [1, 2, 20, 100]) {
    const lines = output.render(width);
    assert.ok(lines.length <= 2_000);
    assert.ok(lines.every(line => visibleWidth(line) <= width));
    assert.ok(plain(lines).replaceAll('\n', '').endsWith('EXIT_7'));
  }
});

test('render owns rejected and synchronously throwing custom highlighters', async () => {
  const patch = parsePatch('@@ -1 +1 @@\n-old\n+new');
  assert.ok(patch);
  for (const synchronous of [false, true]) {
    const failure = new Error('highlight unavailable');
    const syntax: SyntaxService = {
      active: true,
      failure: undefined,
      dispose() {},
      highlight() {
        if (synchronous) throw failure;
        return Promise.reject(failure);
      },
    };
    for (const view of [
      new SourceView({
        source: 'plain source',
        path: 'a.ts',
        theme: dark,
        syntax,
      }),
      new DiffView({patch, path: 'a.ts', theme: dark, syntax}),
    ]) {
      assert.match(plain(view.render(80)), /plain source|old/);
      await new Promise<void>(resolve => setImmediate(resolve));
      assert.equal(view.failure, failure);
      assert.match(plain(view.render(80)), /plain source|old/);
    }
  }
});

test('render owns throwing redraw callbacks without losing readable content', async () => {
  const patch = parsePatch('@@ -1 +1 @@\n-old\n+new');
  assert.ok(patch);
  const failure = new Error('redraw unavailable');
  const syntax: SyntaxService = {
    active: true,
    failure: undefined,
    dispose() {},
    async highlight(source) {
      return source.split('\n').map(content => [{content, offset: 0}]);
    },
  };
  for (const view of [
    new SourceView({
      source: 'plain source',
      path: 'a.ts',
      theme: dark,
      syntax,
      invalidate() {
        throw failure;
      },
    }),
    new DiffView({
      patch,
      path: 'a.ts',
      theme: dark,
      syntax,
      invalidate() {
        throw failure;
      },
    }),
  ]) {
    view.render(80);
    await new Promise<void>(resolve => setImmediate(resolve));
    assert.equal(view.failure, failure);
    assert.match(plain(view.render(80)), /plain source|old/);
  }
});

test('disposal records cleanup failures exactly once without detached rejection', async () => {
  const failure = new Error('dispose failed');
  let disposals = 0;
  const syntax = createSyntaxService(async () => ({
    async load() {},
    tokenize: () => [],
    dispose() {
      disposals++;
      throw failure;
    },
  }));
  await syntax.highlight('source', 'a.ts', 'dark');
  syntax.dispose();
  syntax.dispose();
  await new Promise<void>(resolve => setImmediate(resolve));
  assert.equal(syntax.failure, failure);
  assert.equal(disposals, 1);
  assert.equal(syntax.active, false);
});

test('preferences allow independent ownership and reject malformed configuration', () => {
  assert.deepEqual(
    parsePreferences({version: 1, tools: ['write'], footer: 'native'}),
    {
      version: 1,
      tools: ['write'],
      otherTools: 'compact',
      thinking: 'compact',
      header: 'compact',
      footer: 'native',
      editor: 'soft',
    },
  );
  assert.deepEqual(
    parsePreferences({version: 1, tools: [], footer: 'native'}).tools,
    [],
  );
  for (const value of [
    {version: 2},
    {version: 1, tools: ['bash', 'bash']},
    {version: 1, tools: ['other']},
    {version: 1, footer: 'hidden'},
    {version: 1, footer: null},
    {version: 1, tools: null},
    {version: 1, unknown: true},
    null,
  ])
    assert.throws(() => parsePreferences(value));
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
