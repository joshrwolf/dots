import assert from 'node:assert/strict';
import {existsSync, mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  initTheme,
  type ExecResult,
  type ExtensionAPI,
  type ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences} from '@earendil-works/pi-tui';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';
import {SessionFixture} from '../dev/sessions.ts';
import register from '../extensions/worker-view/index.ts';
import {viewInRex} from '../extensions/workers/index.ts';

initTheme('dark', false);

type Handler = (event: unknown, ctx: ExtensionContext) => unknown;
interface ViewComponent {
  render(width: number): string[];
  handleInput(data: string): void;
}

const ok: ExecResult = {
  stdout: '',
  stderr: '',
  code: 0,
  killed: false,
  truncated: false,
};

/** The viewer registered against a fake host with the given flags. */
function viewer(
  flags: Readonly<Record<string, string>>,
  exec: (file: string, args: string[]) => Promise<ExecResult>,
) {
  const handlers = new Map<string, Handler>();
  const pi = {
    registerFlag() {},
    getFlag: (name: string) => flags[name],
    getSettings: () => ({}),
    on(name: string, handler: Handler) {
      handlers.set(name, handler);
    },
    exec,
  } as unknown as ExtensionAPI;
  register(pi);
  let component: ViewComponent | undefined;
  let mounts = 0;
  let shutdowns = 0;
  let closed!: () => void;
  const finished = new Promise<void>(resolve => {
    closed = resolve;
  });
  const ctx = {
    mode: 'tui',
    ui: {
      setHeader() {},
      setFooter() {},
      setWorkingVisible() {},
      notify(message: string) {
        assert.fail(message);
      },
      custom(
        factory: (
          tui: unknown,
          theme: unknown,
          bindings: unknown,
          done: () => void,
        ) => ViewComponent,
      ) {
        mounts++;
        return new Promise<void>(resolve => {
          component = factory(
            {terminal: {rows: 24}, requestRender() {}},
            getThemeByName('dark')!,
            {
              matches: (data: string, id: string) =>
                id === 'tui.select.cancel' && data === '\x1b',
            },
            () => {
              resolve();
              closed();
            },
          );
        });
      },
    },
    shutdown() {
      shutdowns++;
    },
  } as unknown as ExtensionContext;
  handlers.get('session_start')!({}, ctx);
  return {
    get component() {
      return component;
    },
    get mounts() {
      return mounts;
    },
    get shutdowns() {
      return shutdowns;
    },
    finished,
    input: () => handlers.get('input')!({text: 'prompt'}, ctx),
    shutdown: () => handlers.get('session_shutdown')!({}, ctx),
  };
}

test('the viewer does nothing in a session started without --worker-session', () => {
  const view = viewer({}, async () => assert.fail('no Rex call'));
  assert.equal(view.mounts, 0);
  assert.equal(view.input(), undefined, 'prompts pass through');
  view.shutdown();
});

test('the viewer titles the view with the worker label and runs its dismiss action on close', async t => {
  const directory = mkdtempSync(path.join(tmpdir(), 'dots-worker-view-'));
  t.after(() => rmSync(directory, {recursive: true, force: true}));
  const file = path.join(directory, 'session.jsonl');
  new SessionFixture(file, '/fixture').user('Run the fixture check');
  for (const failing of [false, true]) {
    const calls: string[][] = [];
    const view = viewer(
      {
        'worker-session': file,
        'worker-label': 'Fixture reviewer',
        dismiss: 'views_dismiss',
      },
      async (_file, args) => {
        calls.push(args);
        return failing ? {...ok, code: 1, stderr: 'no server'} : ok;
      },
    );
    try {
      assert.equal(view.mounts, 1);
      assert.deepEqual(view.input(), {action: 'handled'});
      const shown = stripTerminalSequences(
        view.component!.render(100).join('\n'),
      );
      assert.match(shown, /Worker · Fixture reviewer/);
      assert.match(shown, /Run the fixture check/);
      view.component!.handleInput('q');
      await view.finished;
      assert.deepEqual(calls, [
        ['--timeout', '30s', 'do', 'views_dismiss', '--json', '--args', '{}'],
      ]);
      assert.equal(view.shutdowns, 1, 'a failed dismiss still leaves Pi');
    } finally {
      view.shutdown();
    }
  }
});

test('opening a worker in Rex sends the workers_view action the viewer it ships', async () => {
  const calls: {file: string; args: string[]}[] = [];
  let result = ok;
  const pi = {
    exec: async (file: string, args: string[]) => {
      calls.push({file, args});
      return result;
    },
  } as unknown as ExtensionAPI;
  await viewInRex(pi, 'sessions/w-1.jsonl', 'Review', {
    REX_BIN_DIR: '/rex/bin',
  });
  const [call] = calls;
  assert.equal(call?.file, '/rex/bin/rex');
  assert.deepEqual(call.args.slice(0, -1), [
    '--timeout',
    '30s',
    'do',
    'workers_view',
    '--json',
    '--args',
  ]);
  const args = JSON.parse(call.args.at(-1)!);
  assert.deepEqual(Object.keys(args).sort(), [
    'label',
    'session_file',
    'viewer',
  ]);
  assert.equal(args.session_file, path.resolve('sessions/w-1.jsonl'));
  assert.equal(args.label, 'Review');
  assert.ok(
    path.isAbsolute(args.viewer),
    'the Lua action requires an absolute viewer',
  );
  assert.ok(existsSync(args.viewer), 'the viewer extension exists');
  assert.match(args.viewer, /extensions\/worker-view\/index\.ts$/);
  result = {...ok, code: 1, stderr: 'no action workers_view'};
  await assert.rejects(
    viewInRex(pi, '/s.jsonl', 'Review', {}),
    /no action workers_view/,
  );
  assert.equal(calls.at(-1)?.file, 'rex');
});
