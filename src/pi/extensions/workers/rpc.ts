import {type ChildProcess, spawn} from 'node:child_process';
import {
  Requests,
  asError,
  decodeDisposition,
  decodeRecord,
  decodeState,
  jsonLines,
  type Disposition,
  type UiAnswer,
  type WorkerCommand,
  type WorkerEvent,
  type WorkerState,
} from './protocol.ts';

// A Pi child in RPC mode: strict JSONL over stdio, as documented in Pi's rpc.md.
// Pi's exported RpcClient echoes child stderr to ours and keeps it unbounded,
// so this speaks the same protocol through the shared worker framing.

export interface PiProcessOptions {
  cwd: string;
  args: string[];
  /** Added to this process's environment; an undefined value removes the variable. */
  env?: Record<string, string | undefined>;
  /** Command to run; default is the `pi` launcher on PATH. */
  command?: readonly string[] | undefined;
  /** Deadline for command acknowledgement, not model completion. */
  requestTimeoutMs?: number;
}

const STDERR_CHUNKS = 200;

/** The managed `pi` launcher, never a resolved package path: our extension's own
 * node_modules can shadow the running installation. */
function defaultCommand(): readonly [string, ...string[]] {
  return ['pi'];
}

/** `base` with `overrides` applied; an undefined override removes the variable. */
function childEnv(
  base: NodeJS.ProcessEnv,
  overrides: Record<string, string | undefined> = {},
): NodeJS.ProcessEnv {
  const env = {...base};
  for (const [key, value] of Object.entries(overrides)) {
    if (value === undefined) delete env[key];
    else env[key] = value;
  }
  return env;
}

/** A worker's Pi: one headless child per worker run, owned by its parent session. */
export class PiProcess {
  private child: ChildProcess | null = null;
  private readonly requests: Requests;
  private readonly listeners = new Set<(record: WorkerEvent) => void>();
  private readonly exitListeners = new Set<() => void>();
  private closing: Promise<void> | undefined;
  private exited = false;
  readonly stderr: string[] = [];
  readonly pid: number | undefined;

  constructor(options: PiProcessOptions) {
    this.requests = new Requests('w', options.requestTimeoutMs ?? 60_000);
    const [command, ...prefix] = options.command ?? defaultCommand();
    if (command === undefined) throw new Error('worker command is empty');
    const child = spawn(
      command,
      [...prefix, '--mode', 'rpc', ...options.args],
      {
        cwd: options.cwd,
        env: childEnv(process.env, options.env),
        stdio: ['pipe', 'pipe', 'pipe'],
      },
    );
    this.child = child;
    this.pid = child.pid;
    child.stdout.setEncoding('utf8');
    child.stdout.on(
      'data',
      jsonLines(value => this.read(value)),
    );
    child.stderr.setEncoding('utf8');
    child.stderr.on('data', (chunk: string) => this.diagnose(chunk));
    // Node can emit `exit` after `error`; the first one ends this process.
    child.once('exit', (code, signal) =>
      this.ended(new Error(`pi exited (${signal ?? code})`)),
    );
    child.once('error', error => this.ended(error));
    // A write after the child closed its end (EPIPE, write-after-end) must not
    // become an uncaught exception in the parent; exit reports the failure.
    child.stdin.on('error', () => {});
  }

  get alive(): boolean {
    return !this.closing && this.child !== null && this.child.exitCode === null;
  }

  onRecord(listener: (record: WorkerEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  onExit(listener: () => void): () => void {
    this.exitListeners.add(listener);
    return () => this.exitListeners.delete(listener);
  }

  /** Starts an idle child, or with `steer` steers a busy one; Pi decides atomically. */
  async prompt(
    message: string,
    streamingBehavior?: 'steer',
  ): Promise<Disposition> {
    return decodeDisposition(
      await this.send({
        type: 'prompt',
        message,
        ...(streamingBehavior ? {streamingBehavior} : {}),
      }),
    );
  }

  async state(): Promise<WorkerState> {
    return decodeState(await this.send({type: 'get_state'}));
  }

  /** Answer a blocking extension_ui_request from the child. */
  respondUi(id: string, answer: UiAnswer): void {
    const stdin = this.child?.stdin;
    if (this.closing || !stdin?.writable)
      throw new Error('pi process is not running');
    stdin.write(
      `${JSON.stringify({type: 'extension_ui_response', id, ...answer})}\n`,
    );
  }

  /** Orderly shutdown: closing stdin asks Pi to dispose and exit; escalate if it lingers. */
  close(graceMs = 5000): Promise<void> {
    if (this.closing) return this.closing;
    const child = this.child;
    if (!child) return Promise.resolve();
    this.requests.failAll(new Error('pi process is closing'));
    this.closing = new Promise<void>(resolve => {
      let hardKill: NodeJS.Timeout | undefined;
      const timer = setTimeout(() => {
        child.kill('SIGTERM');
        hardKill = setTimeout(() => child.kill('SIGKILL'), 1000);
      }, graceMs);
      const finish = () => {
        clearTimeout(timer);
        if (hardKill) clearTimeout(hardKill);
        child.removeListener('exit', finish);
        child.removeListener('error', finish);
        resolve();
      };
      child.once('exit', finish);
      child.once('error', finish);
      child.stdin?.end();
    });
    return this.closing;
  }

  private send(command: WorkerCommand): Promise<unknown> {
    const stdin = this.child?.stdin;
    if (this.closing || !stdin?.writable)
      return Promise.reject(new Error('pi process is not running'));
    return this.requests.send(command, (line, fail) => {
      stdin.write(line, error => {
        if (error) fail(asError(error));
      });
    });
  }

  private read(value: unknown): void {
    const record = decodeRecord(value);
    if (!record) return;
    if (record.type === 'response') {
      this.requests.answer(record);
      return;
    }
    for (const listener of [...this.listeners]) {
      try {
        listener(record);
      } catch (error) {
        this.diagnose(`listener error: ${asError(error).message}\n`);
      }
    }
  }

  private ended(error: Error): void {
    if (this.exited) return;
    this.exited = true;
    this.requests.failAll(error);
    this.child = null;
    for (const listener of [...this.exitListeners]) {
      try {
        listener();
      } catch (failure) {
        this.diagnose(`exit listener error: ${asError(failure).message}\n`);
      }
    }
  }

  private diagnose(text: string): void {
    this.stderr.push(text);
    if (this.stderr.length > STDERR_CHUNKS) this.stderr.shift();
  }
}
