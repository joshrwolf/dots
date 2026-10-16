import { type ChildProcess, spawn } from "node:child_process";

// A Pi child in RPC mode: strict JSONL over stdio, as documented in Pi's rpc.md.
// Pi's exported RpcClient echoes child stderr to ours and hides the writer we
// need for extension_ui_response, so this is the same protocol in ~100 lines.

export type RpcRecord = Record<string, unknown> & { type: string };

export interface PiProcessOptions {
  cwd: string;
  args: string[];
  env?: Record<string, string>;
  /** Command to run; default is the `pi` launcher on PATH (or $DOTS_PI_BIN). Tests pass a script. */
  command?: string[];
}

/** The managed `pi` launcher, never a resolved package path: our extension's own
 * node_modules can shadow the running installation. */
export function defaultCommand(env: NodeJS.ProcessEnv = process.env): string[] {
  return [env.DOTS_PI_BIN || "pi"];
}

export class PiProcess {
  private child: ChildProcess | null = null;
  private pending = new Map<string, { resolve: (value: RpcRecord) => void; reject: (error: Error) => void }>();
  private listeners = new Set<(record: RpcRecord) => void>();
  private exitListeners = new Set<(code: number | null, signal: NodeJS.Signals | null) => void>();
  private sequence = 0;
  private buffer = "";
  readonly stderr: string[] = [];
  readonly pid: number | undefined;
  readonly options: PiProcessOptions;

  constructor(options: PiProcessOptions) {
    this.options = options;
    const [command, ...prefix] = options.command ?? defaultCommand();
    const child = spawn(command!, [...prefix, "--mode", "rpc", ...options.args], {
      cwd: options.cwd,
      env: { ...process.env, ...options.env },
      stdio: ["pipe", "pipe", "pipe"],
    });
    this.child = child;
    this.pid = child.pid;
    child.stdout!.setEncoding("utf8");
    child.stdout!.on("data", (chunk: string) => this.read(chunk));
    child.stderr!.setEncoding("utf8");
    child.stderr!.on("data", (chunk: string) => {
      this.stderr.push(chunk);
      if (this.stderr.length > 200) this.stderr.shift();
    });
    child.once("exit", (code, signal) => {
      const error = new Error(`pi exited (${signal ?? code})`);
      for (const request of this.pending.values()) request.reject(error);
      this.pending.clear();
      this.child = null;
      for (const listener of this.exitListeners) {
        try {
          listener(code, signal);
        } catch (error) {
          this.stderr.push(`exit listener error: ${(error as Error).message}\n`);
        }
      }
    });
    child.once("error", (error) => {
      for (const request of this.pending.values()) request.reject(error);
      this.pending.clear();
      this.child = null;
      for (const listener of this.exitListeners) listener(null, null);
    });
    // A write after the child closed its end (EPIPE, write-after-end) must not
    // become an uncaught exception in the parent.
    child.stdin!.on("error", () => {});
  }

  get alive(): boolean {
    return this.child !== null && this.child.exitCode === null;
  }

  onRecord(listener: (record: RpcRecord) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  onExit(listener: (code: number | null, signal: NodeJS.Signals | null) => void): () => void {
    this.exitListeners.add(listener);
    return () => this.exitListeners.delete(listener);
  }

  /** Send a command and resolve with its response record (success or failure). */
  command(command: Record<string, unknown> & { type: string }): Promise<RpcRecord> {
    const id = `w${++this.sequence}`;
    return new Promise((resolve, reject) => {
      if (!this.child?.stdin?.writable) {
        reject(new Error("pi process is not running"));
        return;
      }
      this.pending.set(id, { resolve, reject });
      this.child.stdin.write(`${JSON.stringify({ id, ...command })}\n`, (error) => {
        if (error && this.pending.delete(id)) reject(error);
      });
    });
  }

  async call<T = unknown>(command: Record<string, unknown> & { type: string }): Promise<T> {
    const response = await this.command(command);
    if (response.success === false) throw new Error(String(response.error ?? `${command.type} failed`));
    return response.data as T;
  }

  /** Answer a blocking extension_ui_request from the child. */
  respondUi(id: string, payload: Record<string, unknown>): void {
    if (!this.child?.stdin?.writable) throw new Error("pi process is not running");
    this.child.stdin.write(`${JSON.stringify({ type: "extension_ui_response", id, ...payload })}\n`);
  }

  /** Orderly shutdown: closing stdin asks Pi to dispose and exit; escalate if it lingers. */
  async close(graceMs = 5000): Promise<void> {
    const child = this.child;
    if (!child) return;
    await new Promise<void>((resolve) => {
      let hardKill: NodeJS.Timeout | undefined;
      const timer = setTimeout(() => {
        child.kill("SIGTERM");
        hardKill = setTimeout(() => child.kill("SIGKILL"), 1000);
      }, graceMs);
      child.once("exit", () => {
        clearTimeout(timer);
        if (hardKill) clearTimeout(hardKill);
        resolve();
      });
      child.stdin?.end();
    });
  }

  private read(chunk: string): void {
    this.buffer += chunk;
    let index: number;
    // Split only on LF: U+2028/U+2029 are legal inside JSON strings.
    while ((index = this.buffer.indexOf("\n")) !== -1) {
      let line = this.buffer.slice(0, index);
      this.buffer = this.buffer.slice(index + 1);
      if (line.endsWith("\r")) line = line.slice(0, -1);
      if (!line) continue;
      let record: RpcRecord;
      try {
        record = JSON.parse(line) as RpcRecord;
      } catch {
        continue;
      }
      if (record.type === "response" && typeof record.id === "string" && this.pending.has(record.id)) {
        const request = this.pending.get(record.id)!;
        this.pending.delete(record.id);
        request.resolve(record);
        continue;
      }
      for (const listener of [...this.listeners]) {
        try {
          listener(record);
        } catch (error) {
          this.stderr.push(`listener error: ${(error as Error).message}\n`);
        }
      }
    }
  }
}
