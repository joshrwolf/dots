// Real interactive host in regular and fullscreen TUI modes, through Pi's public
// virtual-terminal seam: package load, resize, theme, reload, the UI
// workbench, goal inspection, the worker transcript viewer, and orderly
// shutdown. No tty, provider, or install.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {test} from 'node:test';
import {hostModule} from '../fixtures/host.ts';

const hostRoot = dirname(
  dirname(dirname(fileURLToPath(hostModule('pi-coding-agent')))),
);
const driver = fileURLToPath(
  new URL('../fixtures/terminal-host.mjs', import.meta.url),
);

async function drive(flow, mode) {
  const work = mkdtempSync(join(tmpdir(), 'dots-pi-term-'));
  try {
    const env = Object.fromEntries(
      Object.entries(process.env).filter(
        ([key]) =>
          !/API_KEY|TOKEN|SECRET|PASSWORD|^DOTS_WORKER_|^REX_/.test(key),
      ),
    );
    Object.assign(env, {
      PI_TEST_HOST_ROOT: hostRoot,
      HOME: work,
      XDG_DATA_HOME: join(work, 'data'),
      XDG_STATE_HOME: join(work, 'state'),
      XDG_CONFIG_HOME: join(work, 'config'),
      TERM: 'xterm-256color',
      PI_CODING_AGENT_DIR: work,
      PI_OFFLINE: '1',
      DOTS_TUI_MARKER: join(work, 'events.log'),
      DOTS_GOAL_MARKER: join(work, 'events.log'),
      // The workbench's initial palette follows the terminal background.
      COLORFGBG: mode === 'regular' ? '0;15' : '15;0',
    });
    const child = spawn(process.execPath, [driver, flow, mode], {
      cwd: work,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let output = '';
    for (const stream of [child.stdout, child.stderr])
      stream.on('data', data => {
        output = (output + data).slice(-16_384);
      });
    const timer = setTimeout(() => child.kill('SIGKILL'), 90_000);
    const [code, signal] = await once(child, 'exit');
    clearTimeout(timer);
    assert.equal(
      code,
      0,
      `${flow}/${mode} exited ${code ?? signal}\n${output}`,
    );
    assert.doesNotMatch(
      output,
      /uncaughtException|UnhandledPromiseRejection|Failed to load extension|ERR_MODULE_NOT_FOUND/,
    );
  } finally {
    rmSync(work, {recursive: true, force: true});
  }
}

for (const flow of ['package', 'workbench', 'goal', 'viewer']) {
  test(`${flow} in both TUI modes`, {concurrency: true}, async t => {
    await Promise.all(
      ['regular', 'fullscreen'].map(mode =>
        t.test(mode, () => drive(flow, mode)),
      ),
    );
  });
}
