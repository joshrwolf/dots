import assert from 'node:assert/strict';
import test, {after} from 'node:test';
import {
  initTheme,
  ToolExecutionComponent,
} from '@earendil-works/pi-coding-agent';
import {
  ProcessTerminal,
  TuiMainScreen,
  stripTerminalSequences,
  visibleWidth,
} from '@earendil-works/pi-tui';
import {toolRenderers} from '../extensions/ui/index.ts';
import {createSyntaxService} from '../src/index.ts';
import {projectCodemode} from '../src/features/tools/codemode.ts';
import type {ToolObservation} from '../src/host/tools.ts';

initTheme('dark', false);
const syntax = createSyntaxService();
after(() => syntax.dispose());
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
const header = {
  type: 'text' as const,
  text: 'Script completed\nWall time 0.1 seconds\nOutput:\n',
};
const call = (name: string, status: string, extra: object = {}) => ({
  id: 'parent/?',
  name,
  args: '{"path":"example.ts"}',
  status,
  ...extra,
});
function row() {
  return new ToolExecutionComponent(
    'codemode',
    'fixture',
    {code: 'const answer = 42;\ntext(answer);'},
    {outputPad: 1},
    toolRenderers(syntax).get('codemode'),
    new TuiMainScreen(new ProcessTerminal()),
    '/workspace',
  );
}
function frame(details: unknown): Extract<ToolObservation, {slot: 'result'}> {
  return {
    slot: 'result',
    args: {},
    details,
    content: [header],
    partial: false,
    error: false,
    durationMs: 1,
    showImages: false,
  };
}

test('codemode transitions through live summaries and replays the same recorded result', () => {
  const live = row();
  assert.match(plain(live.render(100)), /codemode/);
  live.setArgsComplete();
  assert.match(plain(live.render(100)), /codemode/);
  live.markExecutionStarted();
  assert.doesNotMatch(plain(live.render(100)), /running/); // Export retains this neutral invocation slot.
  live.updateResult(
    {
      content: [],
      details: {calls: [call('read', 'running'), call('bash', 'running')]},
      isError: false,
    },
    true,
  );
  assert.match(plain(live.render(100)), /2 running/);
  const result = {
    content: [header, {type: 'text' as const, text: 'answer: 42'}],
    details: {
      calls: [
        call('read', 'ok'),
        call('bash', 'error', {error: 'exit 7'}),
        call('models.classify', 'cancelled'),
      ],
    },
    isError: false,
  };
  live.updateResult(result);
  const replay = row();
  replay.updateResult(structuredClone(result));
  assert.equal(plain(live.render(100)), plain(replay.render(100)));
  assert.match(plain(live.render(100)), /\n [▸▾] codemode/); // A caught child error is not a parent failure.
  assert.match(plain(live.render(100)), /bash: exit 7/);
  assert.match(plain(live.render(100)), /1 failed · 1 cancelled/);
  assert.match(plain(live.render(100)), /answer: 42/);
  assert.doesNotMatch(plain(live.render(100)), /output available|done|×/);
  assert.doesNotMatch(plain(live.render(100)), /Script completed/);
  live.setExpanded(true);
  const expanded = plain(live.render(120));
  assert.match(expanded, /nested results are not recorded/);
  assert.match(expanded, /Arguments \(preview\)/);
  assert.match(expanded, /const answer = 42/);
  assert.match(expanded, /answer: 42/);
  for (let width = 1; width <= 100; width++)
    for (const line of live.render(width))
      assert.ok(visibleWidth(line) <= width, `${width}: ${line}`);
});

test('collapsed receipts retain old errors, output paths, and failure tails', () => {
  const component = row();
  component.updateResult({
    content: [
      header,
      {
        type: 'text',
        text: 'ordinary output\n'.repeat(20) + 'Script error: refused',
      },
    ],
    details: {
      calls: [
        call('early-failure', 'error', {error: 'permission denied'}),
        ...Array.from({length: 12}, (_, n) => call(`success-${n}`, 'ok')),
      ],
      fullOutputPath: '/tmp/example-output.txt',
    },
    isError: true,
  });
  const rendered = plain(component.render(120));
  assert.match(rendered, /early-failure: permission denied/);
  assert.match(rendered, /Script error: refused/);
  assert.match(rendered, /Full output: \/tmp\/example-output.txt/);
  assert.match(rendered, /script failed/);
  assert.match(rendered, /1 failed/);
  assert.doesNotMatch(rendered, /done|output available/);
});

test('malformed, oversized, and absent summaries remain honest and bounded', () => {
  assert.equal(projectCodemode(frame(undefined)).available, false);
  assert.equal(projectCodemode(frame({calls: []})).available, true);
  const model = projectCodemode(
    frame({
      calls: [
        null,
        call('unknown', 'new-status'),
        call('bad-args', 'ok', {args: {}}),
        call('safe', 'ok', {cost: NaN, durationMs: -1}),
      ],
    }),
  );
  assert.equal(model.unreadable, 3);
  assert.equal(model.calls[0]?.cost, undefined);
  assert.equal(model.calls[0]?.durationMs, undefined);
  const large = projectCodemode(
    frame({
      calls: Array.from({length: 300}, () =>
        call('example', 'ok', {args: 'x'.repeat(10_000)}),
      ),
    }),
  );
  assert.equal(large.total, 300);
  assert.equal(large.calls.length, 256);
  assert.ok(large.calls[0]!.args.length < 2100);
  const component = row();
  component.updateResult({
    content: [],
    details: {calls: Array.from({length: 300}, () => call('example', 'ok'))},
    isError: false,
  });
  assert.match(
    plain(component.render(120)),
    /counts below cover this sample only/,
  );
  component.setExpanded(true);
  assert.ok(component.render(1).length < 3000);
});

test('recorded argument previews stay opaque and reported costs survive collapsed selection', () => {
  const args = '{"path":"abbreviated…';
  assert.equal(
    projectCodemode(frame({calls: [call('read', 'ok', {args})]})).calls[0]
      ?.args,
    args,
  );
  const component = row();
  component.updateResult({
    content: [],
    details: {
      calls: Array.from({length: 6}, () =>
        call('models.classify', 'ok', {cost: 0.01}),
      ),
    },
    isError: false,
  });
  assert.match(plain(component.render(120)), /reported cost \$0\.0600/);
  component.updateResult({content: [], details: {calls: []}, isError: false});
  assert.match(plain(component.render(120)), /0 calls/);
});

test('successful batches use a heading and a thin detail rail, not receipt output', () => {
  const component = row();
  component.updateResult({
    content: [
      header,
      {
        type: 'text',
        text: '==> text 1/2 <==\nSuccessfully replaced 1 block(s)\nsecret output',
      },
    ],
    details: {
      calls: [
        call('read', 'ok', {args: '{"path":"/workspace/src/store.ts"}'}),
        call('edit', 'ok', {
          args: '{"path":"/workspace/src/store.ts","edits":[...',
        }),
        call('bash', 'ok', {args: '{"command":"cargo check","timeout":120}'}),
      ],
    },
    isError: false,
    durationMs: 2300,
  });
  const lines = component.render(160).filter(line => line.trim());
  const rendered = plain(lines);
  assert.equal(lines.length, 2);
  assert.match(plain([lines[0]!]), /codemode.*read · edit · bash.*2.3s/);
  assert.match(plain([lines[1]!]), /src\/store.ts · cargo check/);
  assert.doesNotMatch(
    rendered,
    /Successfully|==>|secret output|\/workspace|\{"command"|×|done|output available/,
  );
  assert.match(plain([lines[1]!]), /^\s*│ /);
  assert.doesNotMatch(plain([lines[0]!]), /│|─|╭|╰/);
  const click = () =>
    component.handleMouse({
      type: 'click',
      button: 'left',
      x: 4,
      y: 1,
      screenX: 4,
      screenY: 1,
      width: 160,
      height: component.render(160).length,
      shift: false,
      alt: false,
      ctrl: false,
    });
  assert.equal(click()?.handled, true);
  assert.match(plain(component.render(160)), /secret output/);
  assert.equal(click()?.handled, true);
  assert.equal(component.render(160).filter(line => line.trim()).length, 2);
});

test('live receipts show at most two active calls, then settle without overriding expansion', () => {
  const component = row();
  const calls = [
    call('read', 'ok', {args: '{"path":"finished.ts"}'}),
    ...['first.ts', 'second.ts', 'third.ts'].map(path =>
      call('read', 'running', {args: JSON.stringify({path})}),
    ),
  ];
  component.updateResult({content: [], details: {calls}, isError: false}, true);
  const active = plain(component.render(160));
  assert.equal(component.render(160).filter(line => line.trim()).length, 3);
  assert.match(active, /3 running/);
  assert.match(active, /first.ts/);
  assert.match(active, /second.ts/);
  assert.doesNotMatch(active, /finished.ts|third.ts/);
  const settled = {
    content: [header],
    details: {calls: calls.map(call => ({...call, status: 'ok'}))},
    isError: false,
  };
  component.updateResult(settled);
  assert.equal(component.render(160).filter(line => line.trim()).length, 2);
  assert.match(plain(component.render(160)), /4 reads/);
  assert.doesNotMatch(plain(component.render(160)), /done/);
  component.updateResult({content: [], details: {calls}, isError: false}, true);
  component.setExpanded(true);
  component.updateResult(settled);
  assert.match(plain(component.render(160)), /Arguments \(preview\)/);
  assert.match(plain(component.render(160)), /third.ts/);
});

test('subject previews handle escapes and unknown or truncated arguments without inventing data', () => {
  const component = row();
  component.updateResult({
    content: [],
    details: {
      calls: [
        call('read', 'ok', {args: JSON.stringify({path: 'src/a"b.ts'})}),
        call('bash', 'ok', {args: '{"command":"unfinished'}),
        call('custom', 'ok', {args: '{"sensitive":"detail-only"}'}),
        call('edit', 'ok', {args: '{"path":"cut...'}),
      ],
    },
    isError: false,
  });
  const rendered = plain(component.render(160));
  assert.match(rendered, /src\/a"b.ts · bash · custom/);
  assert.match(rendered, /\+1 more/);
  assert.doesNotMatch(rendered, /unfinished|detail-only|cut\.\.\./);
  for (let width = 1; width <= 160; width++) {
    const lines = component.render(width);
    assert.ok(lines.length <= 3);
    for (const line of lines) assert.ok(visibleWidth(line) <= width);
  }
});

test('quoted source is not promoted into a warning', () => {
  const component = row();
  const output =
    "  error: 'Permission denied',\nWARNING: a string in a fixture";
  component.updateResult({
    content: [header, {type: 'text', text: output}],
    details: {calls: [call('read', 'ok')]},
    isError: false,
  });
  assert.equal(component.render(100).filter(line => line.trim()).length, 2);
  assert.doesNotMatch(
    plain(component.render(100)),
    /Permission denied|WARNING|↳|!/,
  );
  component.setExpanded(true);
  assert.match(plain(component.render(100)), /Permission denied/);
  assert.match(plain(component.render(100)), /WARNING/);
});

test('failure previews occupy one row per diagnostic and expansion retains the recorded error', () => {
  const component = row();
  const error =
    'compiler context\n'.repeat(30) + 'AssertionError: expected true';
  component.updateResult({
    content: [],
    details: {calls: [call('bash', 'error', {error})]},
    isError: false,
  });
  for (const width of [20, 40, 80, 160]) {
    const lines = component.render(width);
    assert.equal(lines.filter(line => line.trim()).length, 3);
    for (const line of lines) assert.ok(visibleWidth(line) <= width);
  }
  assert.match(
    plain(component.render(160)),
    /bash: … AssertionError: expected true/,
  );
  assert.doesNotMatch(plain(component.render(160)), /compiler context/);
  const collapsed = component.render(160).filter(line => line.trim());
  assert.match(plain([collapsed[1]!]), /│ bash/);
  assert.match(plain([collapsed[2]!]), /│ ↳ bash:/);
  component.setExpanded(true);
  assert.match(plain(component.render(160)), /compiler context/);
});

test('subjects stay short in wide panes without conflating distinct paths or interpreting shell', () => {
  const component = row();
  const suffix = 'packages/library/src/index.ts';
  const command = 'printf "quoted && command"; ' + 'echo literal; '.repeat(20);
  component.updateResult({
    content: [],
    details: {
      calls: [
        call('read', 'ok', {
          args: JSON.stringify({path: '/home/example/project-a/' + suffix}),
        }),
        call('read', 'ok', {
          args: JSON.stringify({path: '/home/example/project-b/' + suffix}),
        }),
        call('bash', 'ok', {args: JSON.stringify({command})}),
        call('bash', 'ok', {args: JSON.stringify({command: 'cargo check'})}),
      ],
    },
    isError: false,
  });
  const rendered = plain(component.render(300));
  assert.match(rendered, /2 reads · 2 bash calls/);
  assert.match(
    rendered,
    /…\/example\/project-a\/packages\/library\/src\/index.ts/,
  );
  assert.match(
    rendered,
    /…\/example\/project-b\/packages\/library\/src\/index.ts/,
  );
  assert.match(rendered, /printf "quoted && command";/);
  assert.match(rendered, /\+1 more/);
  assert.doesNotMatch(rendered, /\/home\/example|done|×|output available/);
  assert.ok(visibleWidth(component.render(300).at(-1)!.trimEnd()) < 230);
  component.setExpanded(true);
  assert.match(plain(component.render(300)), /project-a/);
  assert.match(plain(component.render(300)), /project-b/);
});

test('batch paths share a directory anchor across different depths', () => {
  const component = row();
  const paths = [
    '/home/example/project/src/features/tools/codemode.ts',
    '/home/example/project/src/ui/card.ts',
    '/home/example/project/src/ui/state.ts',
  ];
  component.updateResult({
    content: [],
    details: {
      calls: paths.map(path =>
        call('read', 'ok', {args: JSON.stringify({path})}),
      ),
    },
    isError: false,
  });
  const rendered = plain(component.render(240));
  assert.match(
    rendered,
    /…\/src\/features\/tools\/codemode.ts · …\/src\/ui\/card.ts · …\/src\/ui\/state.ts/,
  );
  assert.doesNotMatch(rendered, /\/home\/example/);
  component.setExpanded(true);
  for (const path of paths)
    assert.ok(plain(component.render(240)).includes(path));
});

test('unrelated paths do not acquire a false shared anchor', () => {
  const component = row();
  component.updateResult({
    content: [],
    details: {
      calls: [
        call('read', 'ok', {args: '{"path":"/one/project-a/index.ts"}'}),
        call('read', 'ok', {args: '{"path":"/other/project-ab/index.ts"}'}),
      ],
    },
    isError: false,
  });
  const rendered = plain(component.render(160));
  assert.match(rendered, /\/one\/project-a\/index.ts/);
  assert.match(rendered, /\/other\/project-ab\/index.ts/);
  assert.doesNotMatch(rendered, /…\//);
});

test('script-only output remains discoverable and cancellation is not called success', () => {
  const component = row();
  component.updateResult({
    content: [header, {type: 'text', text: 'answer: 42'}],
    details: {calls: []},
    isError: false,
  });
  assert.match(plain(component.render(100)), /0 calls/);
  assert.match(plain(component.render(100)), /answer: 42/);
  assert.doesNotMatch(plain(component.render(100)), /output available/);
  component.setExpanded(true);
  assert.match(plain(component.render(100)), /answer: 42/);
  component.setExpanded(false);
  component.updateResult({
    content: [],
    details: {calls: [call('bash', 'cancelled')]},
    isError: false,
  });
  assert.match(plain(component.render(100)), /1 cancelled/);
  assert.doesNotMatch(plain(component.render(100)), /done/);
});

test('only the exact native header block is removed', () => {
  const input = {
    ...frame({calls: []}),
    content: [{type: 'text' as const, text: 'Script completed\nuser output'}],
  };
  assert.equal(projectCodemode(input).output, 'Script completed\nuser output');
});

test('a streamed script is highlighted once, when complete, and never again for progress or idle frames', async () => {
  const highlighted: string[] = [];
  const counting = createSyntaxService(async () => ({
    async load() {},
    tokenize(source) {
      highlighted.push(source);
      return source.split('\n').map(content => [{content, offset: 0}]);
    },
    dispose() {},
  }));
  after(() => counting.dispose());
  const settle = () => new Promise(resolve => setImmediate(resolve));
  const script = Array.from(
    {length: 40},
    (_, i) => `const value${i} = ${i};`,
  ).join('\n');
  const component = new ToolExecutionComponent(
    'codemode',
    'fixture',
    {code: ''},
    {outputPad: 1},
    toolRenderers(counting).get('codemode'),
    new TuiMainScreen(new ProcessTerminal()),
    '/workspace',
  );
  component.setExpanded(true);
  for (let end = 20; end < script.length; end += 20) {
    component.updateArgs({code: script.slice(0, end)});
    component.render(100);
    await settle();
  }
  component.updateArgs({code: script});
  assert.match(plain(component.render(100)), /const value39 = 39;/);
  await settle();
  assert.equal(highlighted.length, 0);
  component.setArgsComplete();
  component.markExecutionStarted();
  component.render(100);
  await settle();
  assert.deepEqual(highlighted, [script]);
  for (let i = 0; i < 5; i++) {
    component.updateResult(
      {
        content: [],
        details: {calls: [call('read', i < 4 ? 'running' : 'ok')]},
        isError: false,
      },
      i < 4,
    );
    component.render(100);
    component.setExpanded(i % 2 === 0);
    component.render(100);
    await settle();
  }
  component.setExpanded(true);
  component.render(100);
  await settle();
  assert.equal(highlighted.length, 1);
});
