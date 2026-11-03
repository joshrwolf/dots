import assert from 'node:assert/strict';
import {spawn, type ChildProcess} from 'node:child_process';
import {accessSync, constants, mkdirSync, mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {setTimeout as sleep} from 'node:timers/promises';
import {
  Rex,
  type Exec,
  type ExecResult,
  type RexOptions,
} from '../../src/host/rex.ts';
import type {Launch} from '../../src/host/stream.ts';

// A disposable Rex server for acceptance. Every Rex process it starts, and
// every process the code under test starts through it, gets an environment
// built from scratch and an explicit address inside the fixture's own
// directory, so nothing can reach the user's server: no inherited REX_*,
// HOME or config, and no autostart.

/** The directory holding the `rex` CLI on PATH, or undefined when Rex is not installed. */
export function rexDirectory(): string | undefined {
  for (const dir of (process.env.PATH ?? '').split(path.delimiter)) {
    if (!dir) continue;
    try {
      accessSync(path.join(dir, 'rex'), constants.X_OK);
      return dir;
    } catch {
      // Not here.
    }
  }
  return undefined;
}

export class RexServer {
  readonly dir: string;
  readonly socket: string;
  /** The server address the code under test is given as its Rex host. */
  readonly address: string;
  readonly binary: string;
  readonly env: NodeJS.ProcessEnv;
  /** Event streams the code under test launched, newest last. */
  readonly launched: ChildProcess[] = [];
  private server: ChildProcess | undefined;
  private serverExit: Promise<void> | undefined;
  private serverLog = '';

  constructor(binDir: string) {
    // macOS caps Unix socket paths near 104 bytes; long TMPDIRs do not fit.
    const base = tmpdir().length <= 40 ? tmpdir() : '/tmp';
    this.dir = mkdtempSync(path.join(base, 'dots-rex-'));
    for (const sub of ['home/.config', 'state', 'steps'])
      mkdirSync(path.join(this.dir, sub), {recursive: true});
    this.socket = path.join(this.dir, 's.sock');
    this.address = `unix://${this.socket}`;
    this.binary = path.join(binDir, 'rex');
    this.env = {
      HOME: path.join(this.dir, 'home'),
      PATH: ['/usr/bin', '/bin', binDir].join(':'),
      TMPDIR: this.dir,
      XDG_RUNTIME_DIR: this.dir,
      XDG_CONFIG_HOME: path.join(this.dir, 'home/.config'),
    };
    this.owned(this.address);
  }

  /** Refuses any address outside this fixture's directory. */
  owned(address: string | undefined): void {
    assert.ok(
      address !== undefined && address.startsWith('unix://'),
      `Rex address must be a Unix socket: ${address}`,
    );
    const relative = path.relative(this.dir, address.slice('unix://'.length));
    assert.ok(
      relative && !relative.startsWith('..') && !path.isAbsolute(relative),
      `Rex socket ${address} is outside ${this.dir}`,
    );
  }

  /** Every Rex client call must name the binary, disable autostart and target an owned socket. */
  private guard(file: string, args: readonly string[]): void {
    assert.equal(file, this.binary, 'Rex runs from the selected binary');
    assert.equal(args[0], '--autostart=false', 'Rex clients never autostart');
    assert.ok(
      args[1] === '--server' || args[1] === '-S',
      `Rex clients name their server: ${args.join(' ')}`,
    );
    this.owned(args[2]);
  }

  /** The code under test's process runner, isolated and guarded. */
  readonly exec: Exec = (file, args, signal) => {
    this.guard(file, args);
    return run(file, args, this.env, signal);
  };

  /** The code under test's stream launcher, isolated and guarded. */
  readonly launch: Launch = (file, args, options) => {
    this.guard(file, args);
    const child = spawn(file, [...args], {...options, env: this.env});
    this.launched.push(child);
    return child;
  };

  /** A `Rex` pointed at nothing but this server's binary and runners. */
  observer(options: Partial<RexOptions> = {}): Rex {
    return new Rex({
      env: {REX_BIN_DIR: path.dirname(this.binary)},
      exec: this.exec,
      launch: this.launch,
      settleMs: 400,
      ...options,
    });
  }

  /** Runs a Rex client command against this server. */
  async client(...args: string[]): Promise<ExecResult> {
    return this.exec(
      this.binary,
      ['--autostart=false', '--server', this.address, ...args],
      AbortSignal.timeout(15_000),
    );
  }

  /** A control API call that must succeed. */
  async api(method: string, params: object = {}): Promise<unknown> {
    const result = await this.client(
      'api',
      'call',
      method,
      JSON.stringify(params),
    );
    assert.equal(result.code, 0, `${method}: ${result.stderr}`);
    return JSON.parse(result.stdout);
  }

  async instance(): Promise<string> {
    const status = (await this.api('server.status')) as {instance_id: string};
    return status.instance_id;
  }

  /** Start the server, restoring saved sessions only when asked; resolves with its instance ID. */
  async start({restore = false} = {}): Promise<string> {
    assert.equal(this.server, undefined, 'one server at a time');
    const server = spawn(
      this.binary,
      [
        'server',
        'run',
        '--listen',
        this.address,
        '--state-dir',
        path.join(this.dir, 'state'),
        '--config',
        '/dev/null',
        ...(restore ? [] : ['--restore=false']),
      ],
      {cwd: this.dir, env: this.env, stdio: ['ignore', 'pipe', 'pipe']},
    );
    this.server = server;
    this.serverLog = '';
    const log = (chunk: Buffer) => {
      this.serverLog = (this.serverLog + chunk.toString()).slice(-4000);
    };
    server.stdout.on('data', log);
    server.stderr.on('data', log);
    this.serverExit = new Promise(resolve => server.once('close', resolve));
    const deadline = Date.now() + 15_000;
    for (;;) {
      assert.equal(
        server.exitCode,
        null,
        `the Rex server exited: ${this.serverLog}`,
      );
      const result = await this.client('api', 'call', 'server.status');
      if (result.code === 0)
        return (JSON.parse(result.stdout) as {instance_id: string}).instance_id;
      assert.ok(
        Date.now() < deadline,
        `the Rex server did not start: ${result.stderr} ${this.serverLog}`,
      );
      await sleep(100);
    }
  }

  /** Stop the server this fixture started, and only it. */
  async stop(): Promise<void> {
    const server = this.server;
    if (!server) return;
    await this.client('server', 'stop');
    const exited = await Promise.race([
      this.serverExit?.then(() => true),
      sleep(5_000, false),
    ]);
    if (!exited && server.exitCode === null && server.signalCode === null) {
      server.kill('SIGKILL');
      await this.serverExit;
    }
    this.server = undefined;
    this.serverExit = undefined;
  }

  async dispose(): Promise<void> {
    await this.stop();
    for (const child of this.launched)
      if (child.exitCode === null && child.signalCode === null)
        child.kill('SIGKILL');
    rmSync(this.dir, {recursive: true, force: true});
  }

  /** Creates an empty session; resolves with its ID. */
  async session(label: string): Promise<string> {
    const created = (await this.api('session.create', {label})) as {
      session_id: string;
    };
    return created.session_id;
  }

  /**
   * Starts `sh -c script` in a labeled terminal block that stays open after
   * the program exits (`rex run --keep-open`); resolves with the block ID.
   */
  async run(session: string, label: string, script: string): Promise<string> {
    const result = await this.client(
      'run',
      '-s',
      session,
      '--json',
      '--keep-open',
      '--focus=false',
      '--label',
      label,
      '--shell=none',
      '--',
      '/bin/sh',
      '-c',
      script,
    );
    assert.equal(result.code, 0, `rex run: ${result.stderr}`);
    const [block] = (JSON.parse(result.stdout) as {block_ids: string[]})
      .block_ids;
    assert.ok(block, 'rex run created a block');
    return block;
  }

  /** Waits until a block's root status record is in `state`. */
  async reported(session: string, block: string, state: string): Promise<void> {
    await until(`${block} reporting ${state}`, async () => {
      const {records} = (await this.api('block.method', {
        session_id: session,
        block_id: block,
        method: 'program_status',
      })) as {records: {id?: string; state: string}[]};
      return records.some(
        record => (record.id ?? '') === '' && record.state === state,
      );
    });
  }

  /** A block another creator provides, in a new window. */
  async onboarding(session: string, label: string): Promise<string> {
    const created = (await this.api('session.new_window', {
      session_id: session,
      focus: false,
      layout: {
        block: {flavor: 'com.superlogical.onboarding.onboarding', label},
      },
    })) as {block_ids: string[]};
    const [block] = created.block_ids;
    assert.ok(block);
    return block;
  }

  /** Lets a program waiting in `hold NAME` (see `program`) continue. */
  step(name: string): void {
    mkdirSync(path.join(this.dir, 'steps', name));
  }

  /**
   * A minimal program: each line is a shell command; `hold NAME` blocks until
   * the test calls `step(NAME)`, and `report FIELDS` prints an OSC 7501 record.
   */
  program(...lines: string[]): string {
    const steps = path.join(this.dir, 'steps');
    return [
      `hold() { while [ ! -d '${steps}/'"$1" ]; do sleep 0.05; done; }`,
      `report() { printf '\\033]7501;%s\\033\\\\' "$1"; }`,
      ...lines,
    ].join('\n');
  }
}

/** Base64, as OSC 7501 carries its text fields. */
export const b64 = (text: string) => Buffer.from(text).toString('base64');

function run(
  file: string,
  args: readonly string[],
  env: NodeJS.ProcessEnv,
  signal: AbortSignal,
): Promise<ExecResult> {
  return new Promise((resolve, reject) => {
    const child = spawn(file, [...args], {
      env,
      signal,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8').on('data', (chunk: string) => {
      stdout += chunk;
    });
    child.stderr.setEncoding('utf8').on('data', (chunk: string) => {
      stderr += chunk;
    });
    child.once('error', error => {
      if (signal.aborted) resolve({stdout, stderr, code: 1, killed: true});
      else reject(error);
    });
    child.once('close', code =>
      resolve({stdout, stderr, code: code ?? 1, killed: signal.aborted}),
    );
  });
}

/** Polls until `ready` returns a value, or fails after `ms`. */
export async function until<T>(
  what: string,
  ready: () => T | undefined | false | Promise<T | undefined | false>,
  ms = 10_000,
): Promise<T> {
  const deadline = Date.now() + ms;
  for (;;) {
    const value = await ready();
    if (value !== undefined && value !== false) return value;
    assert.ok(Date.now() < deadline, `timed out waiting for ${what}`);
    await sleep(25);
  }
}
