import assert from 'node:assert/strict';
import {homedir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
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
import {pathLabel, urlLabel} from '../src/features/tools/records.ts';

initTheme('dark', false);

test('path labels preserve project identity and component boundaries', () => {
  const home = homedir();
  assert.equal(
    pathLabel(join(home, 'example/src/a.ts'), join(home, 'example')),
    'src/a.ts',
  );
  assert.equal(
    pathLabel(join(home, 'other/src/a.ts'), '/workspace'),
    '~/other/src/a.ts',
  );
  assert.equal(
    pathLabel(home + '-other/a.ts', '/workspace'),
    home + '-other/a.ts',
  );
  assert.equal(pathLabel('../other/a.ts', '/workspace'), '../other/a.ts');
});

test('URL labels expose host, port, path and omitted query/fragment without guessing unknown protocols', () => {
  assert.equal(
    urlLabel('https://example.com:8443/docs/read?q=hello#section'),
    'example.com:8443/docs/read?…#…',
  );
  assert.equal(urlLabel('https://example.com'), 'example.com');
  assert.equal(urlLabel('not a URL'), 'not a URL');
  assert.equal(urlLabel('file:///tmp/example'), 'file:///tmp/example');
});

test('generic URL subjects retain full recorded request on expansion and literal commands stay literal', () => {
  const url = 'https://example.com/docs/日本語?q=fixture#anchor';
  const row = new ToolExecutionComponent(
    'lookup',
    'fixture',
    {url},
    {},
    rendererResolver()('lookup', () => undefined),
    new TuiMainScreen(new ProcessTerminal()),
    '/workspace',
  );
  row.updateResult({
    content: [{type: 'text', text: 'recorded response'}],
    isError: false,
  });
  const plain = (width: number) =>
    stripTerminalSequences(row.render(width).join('\n'));
  assert.ok(
    plain(160).includes('example.com' + new URL(url).pathname + '?…#…'),
  );
  row.setExpanded(true);
  assert.ok(plain(160).includes(url));
  for (let width = 1; width <= 100; width++)
    assert.ok(row.render(width).every(line => visibleWidth(line) <= width));
  const command = 'printf "https://example.com?a=b && literal"';
  const bash = new ToolExecutionComponent(
    'bash',
    'command',
    {command},
    {},
    rendererResolver()('bash', () => undefined),
    new TuiMainScreen(new ProcessTerminal()),
    '/workspace',
  );
  assert.ok(
    stripTerminalSequences(bash.render(160).join('\n')).includes(command),
  );
});
