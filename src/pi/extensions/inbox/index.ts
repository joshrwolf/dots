import {randomUUID} from 'node:crypto';
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import type {
  ExtensionAPI,
  ExtensionContext,
} from '@earendil-works/pi-coding-agent';

/**
 * An inbox through which local tools, such as the dots-diff viewer, hand
 * this interactive session a user message. Each session registers itself in
 * a per-user directory, one file per process, with the socket it listens
 * on. A delivery starts a turn when the session is idle and follows the
 * current run otherwise; the sender gets an ack either way.
 */

/** Largest message accepted, in bytes. */
const MAX_REQUEST = 1024 * 1024;
const REQUEST_TIMEOUT_MS = 5000;

/** Where sessions register: the user's state directory. */
export function inboxDir(env: NodeJS.ProcessEnv): string {
  const state = env.XDG_STATE_HOME || path.join(os.homedir(), '.local/state');
  return path.join(state, 'dots', 'pi-inbox');
}

/** What a sender needs to find and name a session. */
export interface Registration {
  pid: number;
  sessionId: string;
  sessionFile?: string | undefined;
  sessionName?: string | undefined;
  rexSession?: string | undefined;
  rexBlock?: string | undefined;
  cwd: string;
  socket: string;
}

/** How a delivery was taken. */
export type Ack = 'started' | 'queued';

/** Hands the text to the session; throws when it cannot. */
export type Deliver = (text: string) => Ack;

type Reply = {ok: true; state: Ack} | {ok: false; error: string};

/** The reply to one request line. */
export function answer(line: string, deliver: Deliver): Reply {
  let request: unknown;
  try {
    request = JSON.parse(line);
  } catch {
    return {ok: false, error: 'expected one JSON request line'};
  }
  if (
    typeof request !== 'object' ||
    request === null ||
    (request as {type?: unknown}).type !== 'deliver' ||
    typeof (request as {text?: unknown}).text !== 'string'
  )
    return {ok: false, error: 'expected {"type":"deliver","text":"…"}'};
  const text = (request as {text: string}).text;
  if (!text.trim()) return {ok: false, error: 'the message is empty'};
  try {
    return {ok: true, state: deliver(text)};
  } catch (e) {
    return {ok: false, error: e instanceof Error ? e.message : String(e)};
  }
}

/** Whether process `pid` is alive. */
function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return (e as NodeJS.ErrnoException).code === 'EPERM';
  }
}

/** Removes registrations and sockets of processes that are gone. */
export function pruneStale(dir: string): void {
  let names: string[];
  try {
    names = readdirSync(dir);
  } catch {
    return;
  }
  for (const name of names) {
    const match = /^(\d+)\.(json|sock)$/.exec(name);
    if (match && !alive(Number(match[1])))
      rmSync(path.join(dir, name), {force: true});
  }
}

/** A listening inbox and its registration; `close` removes both. */
export class Inbox {
  private readonly server: net.Server;
  readonly registration: Registration;
  private readonly file: string;

  private constructor(
    server: net.Server,
    registration: Registration,
    file: string,
  ) {
    this.server = server;
    this.registration = registration;
    this.file = file;
  }

  static async open(
    dir: string,
    who: Omit<Registration, 'pid' | 'socket'>,
    deliver: Deliver,
  ): Promise<Inbox> {
    mkdirSync(dir, {recursive: true, mode: 0o700});
    pruneStale(dir);
    const socket = path.join(dir, `${process.pid}.sock`);
    rmSync(socket, {force: true});
    const server = net.createServer(conn => serve(conn, deliver));
    await new Promise<void>((resolve, reject) => {
      server.once('error', reject);
      // Owner-only: the socket is created under a restrictive umask.
      const umask = process.umask(0o077);
      try {
        server.listen(socket, () => resolve());
      } finally {
        process.umask(umask);
      }
    });
    const registration: Registration = {...who, pid: process.pid, socket};
    const file = path.join(dir, `${process.pid}.json`);
    const tmp = `${file}.${randomUUID()}.tmp`;
    writeFileSync(tmp, JSON.stringify(registration), {mode: 0o600});
    renameSync(tmp, file);
    return new Inbox(server, registration, file);
  }

  /** Stops listening and unregisters; safe to call twice. */
  close(): void {
    this.server.close();
    // Another process may have taken the pid's files since; only remove ours.
    try {
      const current = JSON.parse(readFileSync(this.file, 'utf8')) as {
        sessionId?: string;
      };
      if (current.sessionId === this.registration.sessionId)
        rmSync(this.file, {force: true});
    } catch {
      // Already gone.
    }
    rmSync(this.registration.socket, {force: true});
  }
}

function serve(conn: net.Socket, deliver: Deliver): void {
  let buffered = '';
  let done = false;
  conn.setEncoding('utf8');
  conn.setTimeout(REQUEST_TIMEOUT_MS, () => conn.destroy());
  const reply = (r: Reply) => {
    done = true;
    conn.end(`${JSON.stringify(r)}\n`);
  };
  conn.on('data', (chunk: string) => {
    if (done) return;
    buffered += chunk;
    const newline = buffered.indexOf('\n');
    if (newline >= 0) reply(answer(buffered.slice(0, newline), deliver));
    else if (buffered.length > MAX_REQUEST)
      reply({ok: false, error: 'the message is too large'});
  });
  conn.on('error', () => conn.destroy());
}

/** Starts a turn when idle; otherwise follows the current run. */
export function deliverTo(
  pi: Pick<ExtensionAPI, 'sendUserMessage'>,
  ctx: Pick<ExtensionContext, 'isIdle'>,
): Deliver {
  return text => {
    if (ctx.isIdle()) {
      pi.sendUserMessage(text);
      return 'started';
    }
    pi.sendUserMessage(text, {deliverAs: 'followUp'});
    return 'queued';
  };
}

export default function (
  pi: ExtensionAPI,
  env: NodeJS.ProcessEnv = process.env,
): void {
  let inbox: Inbox | undefined;
  const close = () => {
    inbox?.close();
    inbox = undefined;
  };
  pi.on('session_start', async (_event, ctx) => {
    close();
    // Workers and headless runs are reached through their owners.
    if (ctx.mode !== 'tui' || env.DOTS_WORKER_ID) return;
    const sm = ctx.sessionManager;
    try {
      inbox = await Inbox.open(
        inboxDir(env),
        {
          sessionId: sm.getSessionId(),
          sessionFile: sm.getSessionFile(),
          sessionName: sm.getSessionName(),
          rexSession: env.REX_SESSION,
          rexBlock: env.REX_BLOCK,
          cwd: sm.getCwd(),
        },
        deliverTo(pi, ctx),
      );
    } catch (e) {
      ctx.ui.notify(
        `Review inbox unavailable: ${e instanceof Error ? e.message : String(e)}`,
        'warning',
      );
    }
  });
  pi.on('session_shutdown', close);
}
