import {spawn, type ChildProcess} from 'node:child_process';
import {systemClock, type Clock} from './clock.ts';

/**
 * The connection of a long-lived child that prints one JSON value per line.
 * `idle` means no consumer wants it; `connected` follows each spawn;
 * `disconnected` carries the end of the child's stderr and the planned retry.
 */
export type StreamState =
  | {readonly status: 'idle'}
  | {readonly status: 'connected'; readonly since: number}
  | {
      readonly status: 'disconnected';
      readonly error: string;
      readonly retryAt: number;
    };

export interface StreamListener {
  /** A newly spawned child is running; values it prints follow. */
  open?(): void;
  /** One parsed line. Lines that are not JSON, or exceed the line cap, are skipped. */
  line(value: unknown): void;
  state?(state: StreamState): void;
}

export type Launch = (
  file: string,
  args: readonly string[],
  options: {stdio: ['ignore', 'pipe', 'pipe']},
) => ChildProcess;

export interface StreamOptions {
  readonly file: string;
  readonly args: readonly string[];
  readonly clock?: Clock;
  readonly launch?: Launch;
  /** First retry delay; doubles per failed connection up to `maxBackoffMs`. */
  readonly minBackoffMs?: number;
  readonly maxBackoffMs?: number;
  /** A connection that lived this long resets the backoff. */
  readonly healthyMs?: number;
  readonly maxLineBytes?: number;
  /** How long a stopped child has between SIGTERM and SIGKILL. */
  readonly stopGraceMs?: number;
}

const STDERR_CHARS = 2_000;

/**
 * One long-lived NDJSON child shared by its subscribers. It starts with the
 * first subscriber, restarts with backoff after the child exits, and stops when
 * the last subscriber leaves. `close` stops it and resolves once every child it
 * started has exited, so session shutdown never abandons a process.
 */
export class NdjsonStream {
  private readonly file: string;
  private readonly args: readonly string[];
  private readonly clock: Clock;
  private readonly launch: Launch;
  private readonly minBackoffMs: number;
  private readonly maxBackoffMs: number;
  private readonly healthyMs: number;
  private readonly maxLineBytes: number;
  private readonly stopGraceMs: number;
  private readonly listeners = new Set<StreamListener>();
  private readonly exits = new Set<Promise<void>>();
  private child: ChildProcess | undefined;
  private retry: unknown;
  private backoff: number;
  private current: StreamState = {status: 'idle'};

  constructor(options: StreamOptions) {
    this.file = options.file;
    this.args = options.args;
    this.clock = options.clock ?? systemClock;
    this.launch =
      options.launch ??
      ((file, args, spawnOptions) => spawn(file, [...args], spawnOptions));
    this.minBackoffMs = options.minBackoffMs ?? 1_000;
    this.maxBackoffMs = options.maxBackoffMs ?? 60_000;
    this.healthyMs = options.healthyMs ?? 30_000;
    this.maxLineBytes = options.maxLineBytes ?? 1024 * 1024;
    this.stopGraceMs = options.stopGraceMs ?? 2_000;
    this.backoff = this.minBackoffMs;
  }

  get state(): StreamState {
    return this.current;
  }

  /** Whether a child is running or about to be retried. */
  get active(): boolean {
    return this.child !== undefined || this.retry !== undefined;
  }

  subscribe(listener: StreamListener): () => void {
    this.listeners.add(listener);
    if (!this.active) this.start();
    else listener.state?.(this.current);
    return () => {
      if (this.listeners.delete(listener) && this.listeners.size === 0)
        this.stop();
    };
  }

  /** Stop without restarting and wait for every child this stream started to exit. */
  async close(): Promise<void> {
    this.listeners.clear();
    this.stop();
    await Promise.allSettled([...this.exits]);
  }

  private publish(state: StreamState): void {
    this.current = state;
    for (const listener of [...this.listeners]) listener.state?.(state);
  }

  private start(): void {
    const started = this.clock.now();
    let child: ChildProcess;
    try {
      child = this.launch(this.file, this.args, {
        stdio: ['ignore', 'pipe', 'pipe'],
      });
    } catch (error) {
      this.ended(
        started,
        error instanceof Error ? error.message : String(error),
      );
      return;
    }
    this.child = child;
    let stderr = '';
    let pending = '';
    let discarding = false;
    const owned = () => this.child === child;
    child.stdout?.setEncoding('utf8');
    child.stderr?.setEncoding('utf8');
    child.stderr?.on('data', (chunk: string) => {
      stderr = (stderr + chunk).slice(-STDERR_CHARS);
    });
    child.stdout?.on('data', (chunk: string) => {
      if (!owned()) return;
      const parts = (pending + chunk).split('\n');
      pending = parts.pop() ?? '';
      for (const part of parts) {
        if (discarding) {
          discarding = false;
          continue;
        }
        this.deliver(part);
        if (!owned()) return;
      }
      if (Buffer.byteLength(pending) > this.maxLineBytes) {
        pending = '';
        discarding = true;
      }
    });
    child.once('spawn', () => {
      if (!owned()) return;
      this.publish({status: 'connected', since: this.clock.now()});
      for (const listener of [...this.listeners]) listener.open?.();
    });
    const exit = new Promise<void>(resolve => {
      let done = false;
      const finish = (reason: string) => {
        if (done) return;
        done = true;
        if (owned()) {
          this.child = undefined;
          this.ended(started, stderr.trim() || reason);
        }
        resolve();
      };
      child.on('error', error => finish(error.message));
      child.once('close', (code, signal) =>
        finish(signal ? `terminated by ${signal}` : `exited with code ${code}`),
      );
    });
    this.exits.add(exit);
    const forget = () => this.exits.delete(exit);
    exit.then(forget, forget);
  }

  private deliver(text: string): void {
    const line = text.trim();
    if (!line) return;
    let value: unknown;
    try {
      value = JSON.parse(line);
    } catch {
      return;
    }
    for (const listener of [...this.listeners]) listener.line(value);
  }

  private ended(started: number, error: string): void {
    if (this.listeners.size === 0) {
      this.publish({status: 'idle'});
      return;
    }
    const now = this.clock.now();
    if (now - started >= this.healthyMs) this.backoff = this.minBackoffMs;
    const delay = this.backoff;
    this.backoff = Math.min(this.backoff * 2, this.maxBackoffMs);
    this.retry = this.clock.setTimeout(() => {
      this.retry = undefined;
      if (this.listeners.size) this.start();
    }, delay);
    this.publish({status: 'disconnected', error, retryAt: now + delay});
  }

  private stop(): void {
    if (this.retry !== undefined) this.clock.clearTimeout(this.retry);
    this.retry = undefined;
    this.backoff = this.minBackoffMs;
    const child = this.child;
    this.child = undefined;
    if (this.current.status !== 'idle') this.publish({status: 'idle'});
    if (!child) return;
    // Node reaps before reporting exit; never signal a reaped PID again.
    const exited = () => child.exitCode !== null || child.signalCode !== null;
    if (exited()) return;
    child.kill('SIGTERM');
    const hard = this.clock.setTimeout(() => {
      if (!exited()) child.kill('SIGKILL');
    }, this.stopGraceMs);
    const clear = () => this.clock.clearTimeout(hard);
    child.once('close', clear);
    child.once('error', clear);
  }
}
