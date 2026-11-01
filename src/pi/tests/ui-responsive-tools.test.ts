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
import {rendererResolver} from '../extensions/ui/index.ts';
import {createSyntaxService} from '../src/index.ts';
import {activityHeader} from '../src/ui/card.ts';

initTheme('dark', false);
const syntax = createSyntaxService();
after(() => syntax.dispose());
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
const tui = new TuiMainScreen(new ProcessTerminal());
function tool(
  name: string,
  args: unknown,
  text: string,
  details?: unknown,
  error = false,
) {
  const component = new ToolExecutionComponent(
    name,
    'fixture',
    args,
    {outputPad: 1},
    rendererResolver(syntax)(name, () => undefined),
    tui,
    '/workspace',
  );
  component.setArgsComplete();
  component.markExecutionStarted();
  component.updateResult({
    content: [{type: 'text', text}],
    details,
    isError: error,
  });
  return component;
}
function resize(
  component: ToolExecutionComponent,
  narrow: number,
  wide: number,
): [string, string, string] {
  const render = (width: number) => {
    const lines = component.render(width);
    assert.ok(
      lines.every(line => visibleWidth(line) <= width),
      `${width}: ${plain(lines)}`,
    );
    return plain(lines);
  };
  return [render(narrow), render(wide), render(narrow)];
}

test('bash and background headers fit full commands when there is room, while preserving outcome priority', () => {
  const command =
    'pnpm --filter shipping-component test -- --file=src/日本語/preview.ts';
  const bash = tool(
    'bash',
    {command},
    'Command exited with code 7',
    undefined,
    true,
  );
  const [narrow, wide, narrowAgain] = resize(bash, 44, 180);
  assert.equal(narrow, narrowAgain);
  const narrowHeader = narrow.split('\n').find(line => line.includes('bash'))!;
  const wideHeader = wide.split('\n').find(line => line.includes('bash'))!;
  assert.match(narrowHeader, /exit 7/);
  assert.match(narrowHeader, /…|\.\.\./);
  assert.match(
    wideHeader,
    /pnpm --filter shipping-component test -- --file=src\/日本語\/preview\.ts +exit 7/,
  );
  assert.doesNotMatch(wideHeader, /…|\uFFFD/);
  bash.setExpanded(true);
  const expanded = resize(bash, 36, 180);
  assert.equal(expanded[0], expanded[2]);
  assert.ok(expanded[0]!.split('\n').length > expanded[1]!.split('\n').length);
  assert.match(expanded[0]!, /preview\.ts/);
  const wake = tool('wake', {text: command}, 'scheduled', {
    text: command,
    active: true,
    in: '15m',
  });
  const [small, large, smallAgain] = resize(wake, 42, 180);
  assert.equal(small, smallAgain);
  const smallHeader = small.split('\n').find(line => line.includes('wake'))!;
  const largeHeader = large.split('\n').find(line => line.includes('wake'))!;
  assert.match(smallHeader, /in 15m/);
  assert.match(
    largeHeader,
    /schedule pnpm --filter shipping-component test -- --file=src\/日本語\/preview\.ts/,
  );
  assert.doesNotMatch(largeHeader, /…/);
});

test('long tool headers retain a right gutter without limiting expanded bodies', () => {
  const component = tool('bash', {command: 'echo ' + 'x'.repeat(500)}, 'ok');
  for (const width of [40, 80, 160, 240]) {
    const lines = component.render(width);
    const header = lines.find(line =>
      stripTerminalSequences(line).includes('bash'),
    )!;
    assert.ok(visibleWidth(header.trimEnd()) <= width - 4);
    assert.ok(visibleWidth(header.trimEnd()) >= width - 8);
  }
  component.setExpanded(true);
  assert.ok(component.render(80).some(line => visibleWidth(line) > 76));
});

test('tool disclosure icons add no rows and yield to execution failures', () => {
  for (const name of ['read', 'write', 'edit', 'bash']) {
    const component = tool(
      name,
      {path: 'sample.ts', content: 'one line', command: 'echo hello'},
      'one line',
    );
    assert.match(plain(component.render(100)), new RegExp('▸ ' + name));
    assert.equal(component.render(100).filter(line => line.trim()).length, 1);
    component.setExpanded(true);
    assert.match(plain(component.render(100)), new RegExp('▾ ' + name));
    component.updateResult({
      content: [{type: 'text', text: 'fixture failure'}],
      isError: true,
    });
    assert.match(plain(component.render(100)), new RegExp('✗ ' + name));
    assert.doesNotMatch(plain(component.render(100)), /[▸▾]/);
    for (let width = 1; width <= 100; width++)
      assert.ok(
        component.render(width).every(line => visibleWidth(line) <= width),
      );
  }
});

test('header preparation keeps a truthful resource bound separate from terminal clipping', () => {
  const command = 'a'.repeat(8191) + '👩🏽‍💻' + ' tail';
  const subject = activityHeader('bash', command, []).start[1]!.text;
  assert.match(subject, /… \[display limited\]$/);
  assert.doesNotMatch(subject, /\uFFFD|[\uD800-\uDFFF]$/);
  assert.equal(
    activityHeader('bash', 'echo complete', []).start[1]!.text,
    'echo complete',
  );
});

test('nested codemode paths use recorded complete tokens and upstream-limited previews stay explicit', () => {
  const path = '/workspace/packages/shipping-component/src/日本語/preview.ts';
  const args = JSON.stringify({path});
  const truncated =
    JSON.stringify({command: 'pnpm test --'.repeat(30)}).slice(0, 197) + '...';
  const component = tool(
    'codemode',
    {code: 'text("fixture")'},
    'fixture result',
    {
      calls: [
        {name: 'read', args, status: 'ok'},
        {
          name: 'bash',
          args: truncated,
          status: 'error',
          error: 'fixture failure',
        },
      ],
    },
  );
  const [small, large, smallAgain] = resize(component, 52, 240);
  assert.equal(small, smallAgain);
  assert.match(large, /packages\/shipping-component\/src\/日本語\/preview\.ts/);
  assert.doesNotMatch(large, /…\/日本語\/preview/);
  assert.match(small, /fixture failure/);
  component.setExpanded(true);
  const expanded = resize(component, 52, 240);
  assert.equal(expanded[0], expanded[2]);
  assert.match(expanded[1]!, /Arguments \(upstream-truncated preview\)/);
  assert.match(expanded[1]!, /nested results are not recorded/);
  assert.doesNotMatch(expanded[1]!, /--tail=not-recorded/);
});
