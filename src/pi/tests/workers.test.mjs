import assert from 'node:assert/strict';
import {
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {activityRows} from '../src/ui/policy.ts';
import {CHILD_CONTRACT, loadConfig} from '../extensions/workers/config.ts';
import {
  buildCatalog,
  describeCatalog,
  resolveChoice,
} from '../extensions/workers/models.ts';
import {PiProcess} from '../extensions/workers/rpc.ts';
import {transcriptTail} from '../extensions/workers/tail.ts';

// A stand-in for `pi --mode rpc`: answers commands, emits a question and a
// settled run, and leaves a U+2028 inside a string to prove LF-only framing.
const FAKE_PI = `
let buffer = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buffer += chunk;
  let i;
  while ((i = buffer.indexOf("\\n")) !== -1) {
    const line = buffer.slice(0, i); buffer = buffer.slice(i + 1);
    const cmd = JSON.parse(line);
    const out = (o) => process.stdout.write(JSON.stringify(o) + "\\n");
    if (cmd.type === "prompt" && cmd.message === "ignore") continue;
    if (cmd.type === "prompt" && cmd.message === "fail") {
      out({ id: cmd.id, type: "response", command: cmd.type, success: false, error: "Prompt rejected" });
    } else if (cmd.type === "prompt" && cmd.message === "odd") {
      out({ id: cmd.id, type: "response", command: cmd.type, success: true, data: { disposition: "maybe" } });
    } else if (cmd.type === "prompt") {
      out({ id: cmd.id, type: "response", command: "prompt", success: true, data: { disposition: "started" } });
      out({ type: "tool_execution_start", toolCallId: "t1", toolName: "bash", args: {} });
      out({ type: "message_update", usage: { totalTokens: 42 }, assistantMessageEvent: { type: "text_delta", delta: "para\\u2028graph" } });
      out({ type: "extension_ui_request", id: "ui-1", method: "input", title: "Which base?" });
    } else if (cmd.type === "extension_ui_response") {
      global.answer = cmd.value;
      out({ type: "tool_execution_end", toolCallId: "t1", toolName: "bash" });
      out({ type: "message_end", message: { role: "assistant", stopReason: "stop", content: [{ type: "text", text: "done with " + global.answer }] } });
      out({ type: "agent_settled", aborted: false });
    } else if (cmd.type === "get_state") {
      out({ id: cmd.id, type: "response", command: cmd.type, success: true, data: { sessionFile: "/tmp/fake.jsonl", isStreaming: false, sessionId: "fixture" } });
    } else {
      out({ id: cmd.id, type: "response", command: cmd.type, success: true, data: {} });
    }
  }
});
process.stdin.on("end", () => process.exit(0));
process.stderr.write("fake pi started\\n");
process.stdout.write(JSON.stringify({ type: "fixture_ready" }) + "\\n");
process.stdout.write('null\\n[]\\n42\\n{}\\n');
`;

function nextRecord(child, type) {
  return new Promise((resolve, reject) => {
    const unsubscribe = child.onRecord(record => {
      if (record.type !== type) return;
      clearTimeout(watchdog);
      unsubscribe();
      resolve(record);
    });
    const watchdog = setTimeout(() => {
      unsubscribe();
      reject(new Error(`fixture did not emit ${type}`));
    }, 10_000);
  });
}

function fakePi() {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-workers-'));
  const cli = path.join(dir, 'cli.js');
  writeFileSync(cli, FAKE_PI);
  return {dir, cli};
}

test('protocol: correlation, LF-only framing, ui request/response, settle, orderly close', async () => {
  const {dir, cli} = fakePi();
  const child = new PiProcess({
    cwd: dir,
    args: ['--no-session'],
    command: [process.execPath, cli],
  });
  try {
    const records = [];
    child.onRecord(record => records.push(record));
    const question = nextRecord(child, 'extension_ui_request');
    assert.equal(await child.prompt('go'), 'started');
    const ask = await question;
    assert.equal(ask.id, 'ui-1');
    assert.equal(ask.title, 'Which base?');
    assert.ok(
      records.some(
        record =>
          record.type === 'message_update' &&
          record.assistantMessageEvent.delta === 'para\u2028graph',
      ),
    );
    assert.deepEqual(
      records.find(record => record.type === 'message_update').usage,
      {totalTokens: 42},
    );
    assert.deepEqual(
      records.find(record => record.type === 'tool_execution_start'),
      {
        type: 'tool_execution_start',
        toolCallId: 't1',
        toolName: 'bash',
        args: {},
      },
      'decoded to consumed fields',
    );
    const ended = nextRecord(child, 'message_end');
    const settled = nextRecord(child, 'agent_settled');
    child.respondUi('ui-1', {value: 'main'});
    assert.equal((await ended).message.content[0].text, 'done with main');
    assert.deepEqual(await settled, {type: 'agent_settled', aborted: false});
    assert.deepEqual(
      records.find(record => record.type === 'tool_execution_end'),
      {
        type: 'tool_execution_end',
        toolCallId: 't1',
        toolName: 'bash',
        isError: false,
      },
    );
    assert.equal(
      records.some(record => record.type === 'fixture_ready'),
      false,
      'records outside the worker protocol are not delivered',
    );
    assert.deepEqual(await child.state(), {
      sessionFile: '/tmp/fake.jsonl',
      isStreaming: false,
    });
    await assert.rejects(child.prompt('fail'), /Prompt rejected/);
    await assert.rejects(child.prompt('odd'), /unknown disposition/);
    assert.match(child.stderr.join(''), /fake pi started/);
    assert.equal(child.alive, true);
    await child.close(1000);
    assert.equal(child.alive, false);
    await assert.rejects(child.state(), /not running/);
  } finally {
    await child.close(100);
    rmSync(dir, {recursive: true, force: true});
  }
});

test('RPC acknowledgement deadlines clean up and close preempts pending commands', async t => {
  const {dir, cli} = fakePi();
  const child = new PiProcess({
    cwd: dir,
    args: [],
    command: [process.execPath, cli],
    requestTimeoutMs: 30_000,
  });
  try {
    // A real start under load can take longer than a short deadline; only
    // the mocked deadline below is under test.
    await child.state();
    t.mock.timers.enable({apis: ['setTimeout']});
    const expired = assert.rejects(
      child.prompt('ignore'),
      /prompt acknowledgement timed out/,
    );
    t.mock.timers.tick(30_000);
    await expired;
    assert.ok((await child.state()).sessionFile);
    const pending = assert.rejects(child.prompt('ignore'), /closing/);
    const close = child.close(100);
    assert.equal(child.close(100), close, 'close owns one shutdown timer');
    await Promise.all([pending, close]);
  } finally {
    await child.close(100);
    rmSync(dir, {recursive: true, force: true});
  }
});

const header = {
  type: 'session',
  version: 3,
  id: 'tail',
  timestamp: '2026-01-01T00:00:00.000Z',
  cwd: '/fixture',
};
const entry = (id, parentId, value) => ({
  id,
  parentId,
  timestamp: '2026-01-01T00:00:01.000Z',
  ...value,
});
const assistant = (id, parentId, content) =>
  entry(id, parentId, {
    type: 'message',
    message: {role: 'assistant', content, stopReason: 'stop', timestamp: 1},
  });
const lines = values =>
  values.map(value => JSON.stringify(value)).join('\n') + '\n';

test('transcript tails show the active branch with compaction applied, as the transcript view does', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-worker-tail-'));
  const file = path.join(dir, 'session.jsonl');
  try {
    writeFileSync(
      file,
      lines([
        header,
        assistant('1', null, [{type: 'text', text: 'summarized away'}]),
        assistant('2', '1', [
          {type: 'toolCall', id: 'call', name: 'read', arguments: {path: 'a'}},
        ]),
        entry('3', '2', {
          type: 'message',
          message: {
            role: 'toolResult',
            toolCallId: 'call',
            toolName: 'read',
            content: [{type: 'text', text: 'file\ntext'}],
            isError: false,
            timestamp: 1,
          },
        }),
        assistant('4', '3', [{type: 'text', text: 'abandoned branch'}]),
        entry('5', '3', {
          type: 'compaction',
          summary: 'Earlier work.',
          firstKeptEntryId: '2',
          tokensBefore: 100,
        }),
        assistant('6', '5', [{type: 'text', text: 'latest\x1b[31m'}]),
      ]) + '{"type":"message","partial',
    );
    assert.deepEqual(transcriptTail(file), [
      '-> read {"path":"a"}',
      '   <- file text',
      'latest',
    ]);
    assert.deepEqual(transcriptTail(file, 1), ['latest']);
    assert.deepEqual(transcriptTail(file, 0), []);
    assert.deepEqual(transcriptTail(file, 3, 10), [
      '-> read {"',
      '   <- file',
      'latest',
    ]);
    assert.match(transcriptTail(path.join(dir, 'absent'))[0], /not saved yet/);
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});

test('transcript tails read only the end of a large session and say what they skipped', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-worker-tail-'));
  const file = path.join(dir, 'session.jsonl');
  try {
    writeFileSync(
      file,
      lines([
        header,
        assistant('1', null, [{type: 'text', text: 'old'.repeat(500_000)}]),
      ]) +
        'null\n{broken\n' +
        lines([assistant('2', '1', [{type: 'text', text: 'latest'}])]),
    );
    const tail = transcriptTail(file);
    assert.match(tail[0], /older records omitted; inspect the session file/);
    assert.match(tail[0], /2 unreadable records skipped/);
    assert.equal(tail.at(-1), 'latest');
    assert.equal(tail.length, 2);
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});

test('closing during spawn failure resolves and isolates throwing exit listeners', async () => {
  const child = new PiProcess({
    cwd: tmpdir(),
    args: [],
    command: [path.join(tmpdir(), 'dots-nonexistent-pi-executable')],
  });
  child.onExit(() => {
    throw new Error('listener failed');
  });
  await child.close(50);
  assert.equal(child.alive, false);
  assert.match(child.stderr.join(''), /listener failed/);
});

test('worker config holds only limits and rejects unknown keys; the child contract is explicit', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'dots-workers-config-'));
  try {
    const file = path.join(dir, 'workers.json');
    assert.deepEqual(loadConfig(path.join(dir, 'absent.json')), {
      maxDepth: 3,
      widgetLines: activityRows.workers,
    });
    writeFileSync(file, JSON.stringify({maxDepth: 2}));
    assert.deepEqual(loadConfig(file), {
      maxDepth: 2,
      widgetLines: activityRows.workers,
    });
    assert.match(CHILD_CONTRACT, /final message is your complete result/);
    assert.match(CHILD_CONTRACT, /message \{ to: "parent"/);
    assert.match(
      CHILD_CONTRACT,
      /never stop, wrap up, or hand off early because of context length/,
    );
    for (const value of [
      null,
      [],
      {modelRoles: {}},
      {maxDepth: 1.5},
      {widgetLines: -1},
      {widgetLines: 2},
    ]) {
      writeFileSync(file, JSON.stringify(value));
      assert.throws(() => loadConfig(file), /must/);
    }
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});

// Hand-authored catalog entries; real model ids never appear in tests.
const model = (provider, id, extra = {}) => ({
  provider,
  id,
  name: id,
  api: 'fixture',
  baseUrl: '',
  reasoning: true,
  input: ['text'],
  cost: {input: 1, output: 5, cacheRead: 0, cacheWrite: 0},
  contextWindow: 200_000,
  maxTokens: 32_000,
  ...extra,
});
const fast = model('alpha', 'fast', {
  cost: {input: 0.1, output: 0.5, cacheRead: 0, cacheWrite: 0},
  input: ['text', 'image'],
});
const deep = model('beta', 'deep', {
  contextWindow: 1_000_000,
  thinkingLevelMap: {off: null, minimal: null, xhigh: 'xhigh'},
});
const plain = model('alpha', 'plain', {reasoning: false});

test('the catalog follows the session scope, falling back to the current model', () => {
  assert.deepEqual(
    buildCatalog([{model: fast}, {model: deep}, {model: fast}], plain).map(
      e => e.ref,
    ),
    ['alpha/fast', 'beta/deep'],
  );
  assert.deepEqual(
    buildCatalog([], plain).map(e => e.ref),
    ['alpha/plain'],
  );
  assert.deepEqual(buildCatalog([], undefined), []);
  const text = describeCatalog(
    buildCatalog([{model: fast}, {model: deep}], deep),
    {model: deep},
  );
  assert.match(
    text,
    /- alpha\/fast: \$0\.1\/\$0\.5 per M in\/out · 200K context · images · thinking off, minimal, low, medium, high/,
  );
  assert.match(
    text,
    /- beta\/deep \(current\): \$1\/\$5 per M in\/out · 1M context · thinking low, medium, high, xhigh/,
  );
  assert.match(describeCatalog([], {}), /only inherit/);
});

test('model and thinking resolve strictly against the catalog and the chosen model', () => {
  const catalog = buildCatalog(
    [{model: fast}, {model: deep}, {model: plain}],
    fast,
  );
  const parent = {model: fast, thinking: 'minimal'};
  const resolve = (choice, from = parent) =>
    resolveChoice(choice, catalog, from, 'medium');
  assert.deepEqual(resolve({model: 'inherit', thinking: 'inherit'}), {
    ok: true,
    model: 'alpha/fast',
    thinking: 'minimal',
  });
  assert.deepEqual(resolve({model: 'beta/deep', thinking: 'xhigh'}), {
    ok: true,
    model: 'beta/deep',
    thinking: 'xhigh',
  });
  assert.deepEqual(resolve({model: 'beta/deep', thinking: 'default'}), {
    ok: true,
    model: 'beta/deep',
    thinking: 'medium',
  });
  assert.deepEqual(
    resolve({model: 'beta/deep'}),
    {ok: true, model: 'beta/deep', thinking: 'medium'},
    'omitted thinking is default',
  );
  assert.deepEqual(
    resolve({model: 'alpha/plain', thinking: 'default'}),
    {ok: true, model: 'alpha/plain', thinking: 'off'},
    'default clamps to the model',
  );
  assert.match(
    resolve({model: 'beta/deep', thinking: 'inherit'}).error,
    /inherit \(minimal\) is unsupported: beta\/deep supports thinking low, medium, high, xhigh/,
  );
  assert.match(
    resolve({model: 'beta/deep', thinking: 'off'}).error,
    /thinking off is unsupported/,
  );
  assert.match(
    resolve({model: 'alpha/fast', thinking: 'max'}).error,
    /thinking max is unsupported/,
  );
  assert.match(
    resolve({model: 'alpha/fast', thinking: 'loud'}).error,
    /thinking loud is unsupported/,
  );
  assert.match(
    resolve({model: 'gamma/gone', thinking: 'default'}).error,
    /gamma\/gone is not available; choose inherit or one of: alpha\/fast, beta\/deep, alpha\/plain/,
  );
  assert.match(
    resolve({model: 'inherit', thinking: 'default'}, {}).error,
    /no current model/,
  );
  assert.match(
    resolve({model: 'alpha/fast', thinking: 'inherit'}, {model: fast}).error,
    /no current thinking level/,
  );
  // The parent's own model is always inheritable, even outside the scope.
  assert.deepEqual(
    resolveChoice(
      {model: 'inherit', thinking: 'high'},
      [],
      {model: deep},
      'medium',
    ),
    {ok: true, model: 'beta/deep', thinking: 'high'},
  );
});

test('pruning removes worker sessions past retention unless kept', async () => {
  const {pruneSessions, RETENTION_MS} =
    await import('../extensions/workers/index.ts');
  const root = mkdtempSync(path.join(tmpdir(), 'dots-worker-prune-'));
  try {
    const now = Date.now();
    const old = (now - RETENTION_MS - 60_000) / 1000;
    for (const id of ['w-old', 'w-kept', 'w-written', 'w-recent']) {
      mkdirSync(path.join(root, id));
      writeFileSync(path.join(root, id, 'session.jsonl'), '{}\n');
      if (id !== 'w-recent') {
        utimesSync(path.join(root, id), old, old);
        if (id !== 'w-written')
          utimesSync(path.join(root, id, 'session.jsonl'), old, old);
      }
    }
    const removed = await pruneSessions(root, id => id === 'w-kept', now);
    assert.deepEqual(removed, ['w-old']);
    assert.deepEqual(readdirSync(root).sort(), [
      'w-kept',
      'w-recent',
      'w-written',
    ]);
    assert.deepEqual(
      await pruneSessions(path.join(root, 'missing'), () => false),
      [],
    );
  } finally {
    rmSync(root, {recursive: true, force: true});
  }
});
