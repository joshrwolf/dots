import assert from 'node:assert/strict';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {
  Inbox,
  answer,
  deliverTo,
  inboxDir,
  pruneStale,
  type Registration,
} from '../extensions/inbox/index.ts';

/** Sends one line and resolves with the reply line. */
function request(socket: string, line: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const conn = net.connect(socket);
    let reply = '';
    conn.setEncoding('utf8');
    conn.on('connect', () => conn.write(`${line}\n`));
    conn.on('data', (chunk: string) => (reply += chunk));
    conn.on('end', () => resolve(reply.trim()));
    conn.on('error', reject);
  });
}

/** A short directory: socket paths have a small length limit. */
function tempDir(): string {
  return mkdtempSync(path.join(os.tmpdir(), 'inbox-'));
}

test('the inbox follows the state directory', () => {
  assert.equal(
    inboxDir({XDG_STATE_HOME: '/s'}),
    path.join('/s', 'dots', 'pi-inbox'),
  );
  assert.equal(
    inboxDir({}),
    path.join(os.homedir(), '.local/state', 'dots', 'pi-inbox'),
  );
});

test('delivery starts a turn when idle and follows the run otherwise', () => {
  const sent: Array<{text: string; deliverAs?: string | undefined}> = [];
  let idle = true;
  const deliver = deliverTo(
    {
      sendUserMessage: (content, options) => {
        sent.push({text: String(content), deliverAs: options?.deliverAs});
      },
    },
    {isIdle: () => idle},
  );
  assert.equal(deliver('one'), 'started');
  idle = false;
  assert.equal(deliver('two'), 'queued');
  assert.deepEqual(sent, [
    {text: 'one', deliverAs: undefined},
    {text: 'two', deliverAs: 'followUp'},
  ]);
});

test('requests are validated and failures reported', () => {
  const ok = () => 'started' as const;
  assert.deepEqual(answer('{"type":"deliver","text":"hi"}', ok), {
    ok: true,
    state: 'started',
  });
  for (const bad of ['nope', '{"type":"other"}', '{"type":"deliver"}']) {
    assert.equal(answer(bad, ok).ok, false, bad);
  }
  assert.deepEqual(answer('{"type":"deliver","text":"  "}', ok), {
    ok: false,
    error: 'the message is empty',
  });
  assert.deepEqual(
    answer('{"type":"deliver","text":"hi"}', () => {
      throw new Error('session closed');
    }),
    {ok: false, error: 'session closed'},
  );
});

test('an open inbox registers, acknowledges deliveries and unregisters', async () => {
  const dir = tempDir();
  try {
    const received: string[] = [];
    const inbox = await Inbox.open(
      dir,
      {sessionId: 's-1', sessionName: 'review', rexBlock: 'block:b', cwd: '/w'},
      text => {
        received.push(text);
        return 'queued';
      },
    );
    const file = path.join(dir, `${process.pid}.json`);
    const registered = JSON.parse(readFileSync(file, 'utf8')) as Registration;
    assert.deepEqual(registered, {
      sessionId: 's-1',
      sessionName: 'review',
      rexBlock: 'block:b',
      cwd: '/w',
      pid: process.pid,
      socket: path.join(dir, `${process.pid}.sock`),
    });
    assert.equal(statSync(file).mode & 0o777, 0o600);
    assert.equal(statSync(registered.socket).mode & 0o077, 0);
    const text = 'Review\n\n1. `a.rs:3` "quoted"';
    assert.equal(
      await request(
        registered.socket,
        JSON.stringify({type: 'deliver', text}),
      ),
      '{"ok":true,"state":"queued"}',
    );
    assert.deepEqual(received, [text]);
    inbox.close();
    inbox.close();
    assert.equal(existsSync(file), false);
    assert.equal(existsSync(registered.socket), false);
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});

test('registrations of dead processes are pruned', () => {
  const dir = tempDir();
  try {
    // Beyond any real pid.
    const dead = path.join(dir, '2147483646.json');
    const live = path.join(dir, `${process.pid}.json`);
    writeFileSync(dead, '{}');
    writeFileSync(live, '{}');
    pruneStale(dir);
    assert.equal(existsSync(dead), false);
    assert.equal(existsSync(live), true);
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});
