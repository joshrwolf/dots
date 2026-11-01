import {execFile} from 'node:child_process';

// The `dots` command as a JSON client. Every call runs the installed binary
// with `--json`; the store, project resolution, and validation stay with it.

export interface DotsOptions {
  /** Passed as `--directory`, for commands that use checkout context. */
  readonly directory?: string;
  readonly cwd?: string;
  readonly signal?: AbortSignal;
  /** Defaults to 10 s. */
  readonly timeoutMs?: number;
  /** Defaults to this process's environment; the catalog follows `XDG_DATA_HOME`. */
  readonly env?: NodeJS.ProcessEnv;
}

/** A failed `dots` run: it exited with an error (its stderr is the message), or printed something other than JSON. */
export class DotsError extends Error {
  readonly args: readonly string[];
  readonly reason: 'failed' | 'invalid-json';
  readonly stderr: string;
  constructor(
    args: readonly string[],
    reason: DotsError['reason'],
    stderr: string,
    message: string,
    options?: ErrorOptions,
  ) {
    super(message, options);
    this.name = 'DotsError';
    this.args = args;
    this.reason = reason;
    this.stderr = stderr;
  }
}

/** Run `dots [--directory D] --json ...args` and parse its one JSON document. */
export function dots(
  args: readonly string[],
  options: DotsOptions = {},
): Promise<unknown> {
  const argv = [
    ...(options.directory ? ['--directory', options.directory] : []),
    '--json',
    ...args,
  ];
  return new Promise((resolve, reject) => {
    options.signal?.throwIfAborted();
    execFile(
      'dots',
      argv,
      {
        ...(options.cwd ? {cwd: options.cwd} : {}),
        ...(options.signal ? {signal: options.signal} : {}),
        ...(options.env ? {env: options.env} : {}),
        timeout: options.timeoutMs ?? 10_000,
        maxBuffer: 1024 * 1024,
        encoding: 'utf8',
        killSignal: 'SIGKILL',
      },
      (error, stdout, stderr) => {
        if (error) {
          reject(
            new DotsError(
              args,
              'failed',
              stderr,
              stderr.trim().slice(0, 512) || error.message,
              {cause: error},
            ),
          );
          return;
        }
        try {
          resolve(JSON.parse(stdout));
        } catch (cause) {
          reject(
            new DotsError(
              args,
              'invalid-json',
              stderr,
              `dots ${args.slice(0, 2).join(' ')} returned invalid JSON`,
              {cause},
            ),
          );
        }
      },
    );
  });
}
