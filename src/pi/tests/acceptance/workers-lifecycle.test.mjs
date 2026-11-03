import assert from 'node:assert/strict';
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import register from '../../extensions/workers/index.ts';
import {ACTIVITY_EVENT} from '../../src/host/activity.ts';
import {Registry, openRegistry} from '../../extensions/workers/registry.ts';

// The extension's lifecycle against a scripted `pi`: spawn -> event -> wake,
// message in every direction, late settles as updates, wait, reload -> paused
// -> continue, stop. No model, no credentials.
function scriptedPi() {
  const args = process.argv.slice(2);
  if (
    args.includes('--model') &&
    args[args.indexOf('--model') + 1] === 'broken/model'
  ) {
    process.stderr.write('Model not found\n');
    process.exit(2);
  }
  writeFileSync(
    path.join(
      tmpdir(),
      'dots-fake-pi-args-' + (process.env.DOTS_WORKER_ID || 'root'),
    ),
    JSON.stringify(args),
  );
  writeFileSync(
    path.join(
      tmpdir(),
      'dots-fake-pi-env-' + (process.env.DOTS_WORKER_ID || 'root'),
    ),
    JSON.stringify({
      rex: process.env.REX_SESSION ?? null,
      block: process.env.REX_BLOCK ?? null,
      dots: process.env.DOTS_REX_SESSION ?? null,
    }),
  );
  const sessionFile = args.includes('--session')
    ? args[args.indexOf('--session') + 1]
    : args[args.indexOf('--session-dir') + 1] + '/session.jsonl';
  const out = o => process.stdout.write(JSON.stringify(o) + '\n');
  let active = false;
  let pendingSettle;
  const settle = (text, final = true) => {
    active = false;
    global.last = text;
    if (final)
      out({
        type: 'message_end',
        message: {
          role: 'assistant',
          content: [{type: 'text', text}],
          stopReason: 'stop',
          ...(text.startsWith('OUTCOME first line')
            ? {usage: {totalTokens: 60_000, cost: {total: 0.125}}}
            : {}),
        },
      });
    out({type: 'agent_settled', aborted: false});
  };
  let buffer = '';
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', chunk => {
    buffer += chunk;
    let i;
    while ((i = buffer.indexOf('\n')) !== -1) {
      const cmd = JSON.parse(buffer.slice(0, i));
      buffer = buffer.slice(i + 1);
      const ok = data =>
        out({
          id: cmd.id,
          type: 'response',
          command: cmd.type,
          success: true,
          data,
        });
      if (cmd.type === 'prompt') {
        if (cmd.message === 'reject') {
          out({
            id: cmd.id,
            type: 'response',
            success: false,
            error: 'rejected continuation',
          });
          continue;
        }
        if (cmd.message === 'handled') {
          ok({disposition: 'handled'});
          continue;
        }
        if (cmd.message === 'no-ack') {
          active = true;
          continue;
        }
        if (active && cmd.streamingBehavior === 'steer') {
          ok({disposition: 'queued'});
          clearTimeout(pendingSettle);
          pendingSettle = setTimeout(
            () => settle('steered: ' + cmd.message),
            50,
          );
          continue;
        }
        active = true;
        out({type: 'agent_start'});
        if (cmd.message === 'delayed-ack') {
          setTimeout(() => {
            ok({disposition: 'started'});
            settle('delayed done');
          }, 150);
          continue;
        }
        ok({disposition: 'started'});
        if (cmd.message === 'provider-error') {
          out({
            type: 'message_end',
            message: {
              role: 'assistant',
              content: [],
              stopReason: 'error',
              errorMessage: 'provider unavailable',
            },
          });
          settle('', false);
          continue;
        }
        if (cmd.message === 'aborted:partial') {
          out({
            type: 'message_end',
            message: {
              role: 'assistant',
              content: [{type: 'text', text: 'partial answer'}],
              stopReason: 'aborted',
            },
          });
          active = false;
          out({type: 'agent_settled', aborted: true});
          continue;
        }
        if (cmd.message === 'missing:final') {
          active = false;
          out({type: 'agent_settled', aborted: false});
          continue;
        }
        out({
          type: 'tool_execution_start',
          toolCallId: 't',
          toolName: 'bash',
          args: {},
        });
        out({
          type: 'message_update',
          usage: {totalTokens: 7},
          assistantMessageEvent: {type: 'text_delta', delta: 'x'},
        });
        const m = cmd.message;
        global.ignoreAbort = m === 'slow:ignore-abort';
        if (m === 'delay-result') {
          global.delayRead = true;
          setTimeout(() => settle('old result'), 30);
          continue;
        }
        if (m === 'two-questions') {
          out({
            type: 'extension_ui_request',
            id: 'q1',
            method: 'input',
            title: 'first question',
          });
          out({
            type: 'extension_ui_request',
            id: 'q2',
            method: 'input',
            title: 'second question',
          });
          continue;
        }
        if (m === 'crash-question') {
          out({
            type: 'extension_ui_request',
            id: 'dead-question',
            method: 'input',
            title: 'question before crash',
            timeout: 600000,
          });
          setTimeout(() => process.exit(1), 50);
          continue;
        }
        if (m === 'expired-question') {
          out({
            type: 'extension_ui_request',
            id: 'expired',
            method: 'input',
            title: 'expires',
            timeout: 30,
          });
          setTimeout(
            () => out({type: 'tool_execution_end', toolName: 'message'}),
            40,
          );
          continue;
        }
        if (m.startsWith('ask:')) {
          out({
            type: 'extension_ui_request',
            id: 'ui-9',
            method: 'input',
            title: m.slice(4),
          });
          return;
        }
        if (m.startsWith('confirm:')) {
          out({
            type: 'extension_ui_request',
            id: 'ui-c',
            method: 'confirm',
            title: m.slice(8),
          });
          return;
        }
        if (m.startsWith('retry:')) {
          out({
            type: 'auto_retry_start',
            attempt: 1,
            maxAttempts: 3,
            delayMs: 100,
            errorMessage: '529 overloaded\nbody',
          });
          setTimeout(() => {
            out({type: 'auto_retry_end', success: true, attempt: 2});
            settle('recovered');
          }, 300);
          return;
        }
        if (m.startsWith('slow:')) {
          pendingSettle = setTimeout(() => settle('slow done'), 400);
          return;
        }
        if (m.startsWith('long:')) {
          settle(
            'OUTCOME first line\n' +
              Array.from({length: 200}, (_, i) => 'detail line ' + i).join(
                '\n',
              ),
          );
          return;
        }
        if (m.startsWith('spontaneous:')) {
          settle('first answer');
          setTimeout(() => {
            active = true;
            out({type: 'agent_start'});
            setTimeout(() => settle('later chatter'), 80);
          }, 120);
          return;
        }
        if (m.startsWith('headline:')) {
          const thinking = heading =>
            out({
              type: 'message_update',
              assistantMessageEvent: {
                type: 'thinking_end',
                content: `**${heading}**`,
              },
            });
          const progress = text =>
            out({
              type: 'extension_ui_request',
              id: 'headline-note',
              method: 'notify',
              message: 'dots.message:' + JSON.stringify({to: 'parent', text}),
            });
          thinking('Inspecting API');
          if (m === 'headline:long-note') progress('x'.repeat(5000));
          if (m === 'headline:progress') {
            progress('Implementation complete');
            thinking('Checking tests');
            progress('Validation complete');
            thinking('Late reasoning');
          }
          out({
            type: 'message_end',
            message: {
              role: 'assistant',
              stopReason: 'stop',
              content: [
                ...(m === 'headline:stream'
                  ? []
                  : [{type: 'thinking', thinking: '**Final reasoning**'}]),
                {type: 'text', text: 'Headline fixture complete'},
              ],
            },
          });
          settle('Headline fixture complete', false);
          return;
        }
        if (m.startsWith('tell-parent:')) {
          out({
            type: 'extension_ui_request',
            id: 'n1',
            method: 'notify',
            message:
              'dots.message:' +
              JSON.stringify({to: 'parent', text: m.slice(12)}),
          });
          settle('told');
          return;
        }
        if (m.startsWith('tell-sibling:')) {
          const [to, ...rest] = m.slice(13).split(' ');
          out({
            type: 'extension_ui_request',
            id: 'n2',
            method: 'notify',
            message:
              'dots.message:' + JSON.stringify({to, text: rest.join(' ')}),
          });
          settle('relayed');
          return;
        }
        settle('done: ' + m);
      } else if (cmd.type === 'extension_ui_response') {
        if (cmd.id === 'q1') {
          out({type: 'tool_execution_end', toolName: 'message'});
          continue;
        }
        if (cmd.id === 'q2') {
          settle('both answered');
          continue;
        }
        if (cmd.id === 'expired') continue;
        settle(
          cmd.id === 'ui-c'
            ? 'confirmed=' + cmd.confirmed
            : 'answered with ' + cmd.value,
        );
      } else if (cmd.type === 'steer') {
        ok({disposition: 'queued'});
        setTimeout(() => settle('steered: ' + cmd.message), 50);
      } else if (cmd.type === 'abort') {
        if (global.ignoreAbort) return;
        ok({});
        out({type: 'agent_settled', aborted: true});
      } else if (cmd.type === 'get_state') {
        if (global.delayRead && global.last === 'old result') {
          global.delayRead = false;
          writeFileSync(sessionFile + '.reading', 'ready');
          setTimeout(() => ok({sessionFile, isStreaming: active}), 150);
          continue;
        }
        ok({sessionFile, isStreaming: active});
      } else {
        ok({});
      }
    }
  });
  process.stdin.on('end', () => process.exit(0));
}

if (process.env.DOTS_WORKERS_FIXTURE === 'pi') scriptedPi();

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(predicate, ms = 3000) {
  const deadline = Date.now() + ms;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error('timed out waiting');
    await sleep(20);
  }
}

// Neutral catalog entries; a chosen model must be in the session scope.
const fixtureModel = (provider, id) => ({
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
});

function harness(root, session = 'session-owner') {
  const handlers = {};
  const tools = new Map();
  const commands = new Map();
  const messages = [];
  const entries = [];
  const published = [];
  const pi = {
    on: (name, fn) => {
      const prior = handlers[name];
      handlers[name] = prior
        ? (...args) => {
            const earlier = prior(...args);
            return fn(...args) ?? earlier;
          }
        : fn;
    },
    registerTool: tool => tools.set(tool.name, tool),
    registerCommand: (name, command) => commands.set(name, command),
    events: {
      on: () => {},
      emit: (channel, data) => {
        if (channel === ACTIVITY_EVENT) published.push(data);
      },
    },
    registerMessageRenderer: () => {},
    sendMessage: (message, options) => {
      messages.push({message, options});
      entries.push({type: 'custom_message', ...message});
      handlers.message_end?.({message: {role: 'custom', ...message}});
    },
    appendEntry: (customType, data) =>
      entries.push({type: 'custom', customType, data}),
    getSettings: () => ({defaultThinkingLevel: 'medium'}),
    exec: () =>
      Promise.resolve({
        code: 128,
        killed: false,
        stdout: '',
        stderr: 'fatal: not a git repository',
      }),
  };
  register(pi, {command: [process.execPath, fileURLToPath(import.meta.url)]});
  const ctx = {
    cwd: root,
    hasUI: false,
    sessionManager: {
      getBranch: () => entries,
      getEntries: () => entries,
      getSessionId: () => session,
      getSessionFile: () => path.join(root, `${session}.jsonl`),
    },
    model: fixtureModel('fake', 'inherited'),
    thinkingLevel: 'low',
    // Pi's ExtensionContext reports no usage before the first response.
    getContextUsage: () => undefined,
    scopedModels: [
      {model: fixtureModel('fake', 'model')},
      {model: fixtureModel('broken', 'model')},
    ],
  };
  // Spawns name their model and thinking; most cases here inherit both.
  const run = (name, params) =>
    tools
      .get(name)
      .execute(
        'call',
        name === 'worker_spawn'
          ? {model: 'inherit', thinking: 'inherit', ...params}
          : params,
        undefined,
        undefined,
        ctx,
      );
  const call = async (name, params) => (await run(name, params)).details;
  return {
    handlers,
    tools,
    commands,
    messages,
    entries,
    published,
    ctx,
    call,
    run,
  };
}

if (process.env.DOTS_WORKERS_FIXTURE !== 'pi') {
  test('parent questions forward deadlines and sibling relays never pretend to wait', async () => {
    const previous = process.env.DOTS_WORKER_ID;
    process.env.DOTS_WORKER_ID = 'w-test';
    try {
      const h = harness(tmpdir());
      h.ctx.ui = {
        async input(_text, _placeholder, options) {
          assert.equal(options.timeout, 1234);
          return undefined;
        },
        notify() {
          assert.fail('unsupported waiting relay must not be sent');
        },
      };
      assert.match(
        (
          await h.run('message', {
            to: 'parent',
            text: 'Approve?',
            wait: true,
            timeoutMs: 1234,
          })
        ).content[0].text,
        /No decision was approved/,
      );
      await assert.rejects(
        h.run('message', {to: 'w-sibling', text: 'hello', wait: true}),
        /not supported for sibling/,
      );
      let answer;
      h.ctx.ui.input = () =>
        new Promise(resolve => {
          answer = resolve;
        });
      const first = h.run('message', {to: 'parent', text: 'first', wait: true});
      await assert.rejects(
        h.run('message', {to: 'parent', text: 'second', wait: true}),
        /already awaiting/,
      );
      answer('yes');
      await first;
    } finally {
      if (previous === undefined) delete process.env.DOTS_WORKER_ID;
      else process.env.DOTS_WORKER_ID = previous;
    }
  });

  test('worker lifecycle against a scripted pi', {timeout: 60_000}, async t => {
    const intervals = new Map();
    t.mock.method(globalThis, 'setInterval', fn => {
      const handle = {};
      intervals.set(handle, fn);
      return handle;
    });
    t.mock.method(globalThis, 'clearInterval', handle =>
      intervals.delete(handle),
    );
    const root = mkdtempSync(path.join(tmpdir(), 'dots-workers-life-'));
    const saved = {...process.env};
    process.env.DOTS_WORKERS_FIXTURE = 'pi';
    process.env.PI_CODING_AGENT_DIR = path.join(root, 'agent');
    delete process.env.DOTS_WORKER_ID;
    const harnesses = [];
    try {
      const h = harness(root);
      harnesses.push(h);
      let stale = false;
      let renders = 0;
      const warnings = [];
      h.ctx.hasUI = true;
      h.ctx.mode = 'tui';
      Object.defineProperty(h.ctx, 'ui', {
        get() {
          assert.equal(stale, false, 'must not access the old UI after reload');
          return {
            setStatus() {},
            setWidget(_key, factory) {
              factory?.(
                {
                  requestRender() {
                    renders++;
                  },
                },
                {},
              );
            },
            notify(text) {
              warnings.push(text);
            },
            custom(...args) {
              return inspectCustom?.(...args);
            },
          };
        },
      });
      h.handlers.session_start({reason: 'startup'}, h.ctx);
      assert.deepEqual([...h.tools.keys()].sort(), [
        'message',
        'worker_spawn',
        'worker_status',
        'worker_stop',
        'worker_wait',
      ]);
      const spawnTool = h.tools.get('worker_spawn');
      assert.match(
        spawnTool.description,
        /- fake\/model: .*thinking off, minimal, low, medium, high/,
        'session start lists the scoped models',
      );
      assert.deepEqual(
        spawnTool.parameters.properties.model.anyOf.map(s => s.const),
        ['inherit', 'fake/model', 'broken/model'],
      );
      await assert.rejects(
        h.run('worker_spawn', {
          label: 'unscoped',
          task: 'x',
          model: 'other/model',
        }),
        /other\/model is not available/,
      );
      await assert.rejects(
        h.run('worker_spawn', {
          label: 'too-deep',
          task: 'x',
          model: 'fake/model',
          thinking: 'max',
        }),
        /thinking max is unsupported/,
      );

      assert.match(
        (await h.run('worker_wait', {})).content[0].text,
        /No workers/,
      );

      // A headless child never inherits its owner's terminal identity, only the session its work belongs to.
      process.env.REX_SESSION = 'session:owner';
      process.env.REX_BLOCK = 'block:owner';
      // spawn -> completion event wakes the owner with the child's final text
      const spawned = await h.call('worker_spawn', {
        label: 'one',
        task: 'count things',
      });
      delete process.env.REX_SESSION;
      delete process.env.REX_BLOCK;
      const childEnvFile = path.join(
        tmpdir(),
        `dots-fake-pi-env-${spawned.id}`,
      );
      assert.deepEqual(JSON.parse(readFileSync(childEnvFile, 'utf8')), {
        rex: null,
        block: null,
        dots: 'session:owner',
      });
      rmSync(childEnvFile, {force: true});
      assert.ok(
        ['running', 'completed'].includes(spawned.state),
        'a fast child may settle before spawn returns',
      );
      assert.ok(
        intervals.size >= 1,
        'live interactive workers start their observer tick',
      );
      await until(() => h.messages.length === 1);
      // A finished worker stays in the widget a while; the ticker stops once it has aged out.
      assert.equal(
        intervals.size,
        1,
        'a just-finished worker keeps the ticker for its trail',
      );
      const realNow = Date.now;
      Date.now = () => realNow() + 121_000;
      try {
        for (const tick of [...intervals.values()]) tick();
      } finally {
        Date.now = realNow;
      }
      assert.equal(
        intervals.size,
        0,
        'the ticker stops once the last finished worker ages out',
      );
      const inheritedArgs = JSON.parse(
        readFileSync(
          path.join(tmpdir(), `dots-fake-pi-args-${spawned.id}`),
          'utf8',
        ),
      );
      assert.equal(
        inheritedArgs[inheritedArgs.indexOf('--model') + 1],
        'fake/inherited',
      );
      assert.equal(
        inheritedArgs[inheritedArgs.indexOf('--thinking') + 1],
        'low',
      );
      assert.ok(!inheritedArgs.includes('--no-mcp'), 'default tools keep MCP');
      assert.equal(h.messages[0].message.details.kind, 'completed');
      assert.match(h.messages[0].message.content, /done: count things/);
      assert.doesNotMatch(h.messages[0].message.content, /session .*jsonl/);
      assert.ok(h.messages[0].message.details.session);
      assert.deepEqual(h.messages[0].options, {
        triggerTurn: true,
        deliverAs: 'steer',
      });
      const one = await h.call('worker_status', {id: spawned.id});
      assert.ok(one.session.endsWith('session.jsonl'));
      const oneText = (await h.run('worker_status', {id: spawned.id}))
        .content[0].text;
      assert.match(
        oneText,
        /one · completed · age \d+s · \d+ tok · \d+ calls?/,
      );
      assert.match(oneText, /result:\n.*done: count things/s);
      assert.match(
        (await h.run('worker_status', {})).content[0].text,
        /result: done: count things/,
        'overview previews the first result line',
      );
      let inspectionText;
      const inspectCustom = async factory => {
        const view = factory(
          {requestRender() {}, terminal: {rows: 32}},
          {fg: (_tone, text) => text, bold: text => text},
          {},
          () => {},
        );
        inspectionText = view.snapshot?.detail;
      };
      await h.commands.get('workers').handler(spawned.id, h.ctx);
      assert.match(
        inspectionText,
        /Context: unknown context · compaction unknown · 200k capacity · Cost: unknown/,
      );
      assert.match(inspectionText, /Directory: .*dots-workers-life-/);

      // Observation failure must not prevent the authoritative worker lifecycle.
      t.mock.method(
        Registry.prototype,
        'event',
        () => {
          throw new Error('test registry unavailable');
        },
        {times: 1},
      );
      // child asks -> asking event -> message answers it -> completion
      const asker = await h.call('worker_spawn', {
        label: 'asker',
        task: 'ask:Which base?',
      });
      await until(() => h.messages.length === 2);
      assert.equal(h.messages[1].message.details.kind, 'asking');
      assert.equal(warnings.length, 1);
      assert.match(warnings[0], /registry disabled/);
      assert.match(
        (await h.run('worker_wait', {all: true})).content[0].text,
        /asking.*answer it/,
      );
      assert.match(h.messages[1].message.content, /Which base\?/);
      const answered = await h.run('message', {to: asker.id, text: 'main'});
      assert.match(answered.content[0].text, /sent an answer to its question/);
      await until(() => h.messages.length === 3);
      assert.match(h.messages[2].message.content, /answered with main/);

      // child -> parent (non-blocking) and child -> sibling (relayed by the owner)
      await h.call('worker_spawn', {
        label: 'teller',
        task: 'tell-parent:heads up',
      });
      await until(() => h.messages.length === 5);
      assert.deepEqual(
        h.messages
          .slice(3)
          .map(m => m.message.details.kind)
          .sort(),
        ['completed', 'note'],
      );
      // A progress note joins the owner's context quietly: never displayed as a card, never a turn.
      const progress = h.messages
        .slice(3)
        .find(m => m.message.details.kind === 'note');
      assert.match(progress.message.content, /heads up/);
      assert.equal(progress.message.display, false);
      assert.equal(progress.options.triggerTurn, false);
      assert.match(
        (await h.run('worker_status', {})).content[0].text,
        /teller/,
      );
      const before = h.messages.length;
      await h.call('worker_spawn', {
        label: 'gossip',
        task: `tell-sibling:${spawned.id} ping`,
      });
      await until(
        () =>
          h.messages
            .slice(before)
            .some(m => m.message.details.kind === 'relay') &&
          h.messages
            .slice(before)
            .some(m =>
              /done: Message from worker gossip/.test(m.message.content),
            ),
      );
      const relay = h.messages.find(m => m.message.details.kind === 'relay');
      assert.equal(relay.message.details.to, spawned.id);
      assert.deepEqual(relay.options, {triggerTurn: false});
      // the target had finished, so the relayed text continued it in its own session
      await until(() =>
        h.messages.some(m =>
          /done: Message from worker gossip/.test(m.message.content),
        ),
      );

      // a provider retry is a phase the owner sees, and it ends with the run
      const retrying = await h.call('worker_spawn', {
        label: 'retrying',
        task: 'retry:go',
      });
      const status = async () =>
        (await h.run('worker_status', {id: retrying.id})).content[0].text;
      for (
        let at = Date.now();
        !/retrying 1\/3: 529 overloaded/.test(await status());
        await sleep(20)
      ) {
        if (Date.now() - at > 3000) assert.fail('the retry never showed');
      }
      await until(() =>
        h.messages.some(m => m.message.details.id === retrying.id),
      );
      assert.doesNotMatch(
        (await h.run('worker_status', {id: retrying.id})).content[0].text,
        /retrying 1/,
      );

      // a long result reaches the owner bounded; the full text stays on the worker
      const verbose = await h.call('worker_spawn', {
        label: 'verbose',
        task: 'long:go',
      });
      await until(() =>
        h.messages.some(m => m.message.details.id === verbose.id),
      );
      const event = h.messages.find(
        m => m.message.details.id === verbose.id,
      ).message;
      assert.match(event.content, /OUTCOME first line/);
      assert.doesNotMatch(event.content, /detail line 150/);
      assert.match(event.content, /full text: worker_status/);
      assert.equal(event.details.truncated, true);
      assert.ok(event.content.length < 2000);
      assert.match(
        (await h.call('worker_status', {id: verbose.id})).result,
        /detail line 199/,
      );
      await h.commands.get('workers').handler(verbose.id, h.ctx);
      assert.match(
        inspectionText,
        /Context: ~60k context · compaction unknown · 200k capacity · Cost: ~\$0\.13 \(estimate\)/,
      );

      // a settle we did not ask for is an update and keeps the real result
      const chatty = await h.call('worker_spawn', {
        label: 'chatty',
        task: 'spontaneous:go',
      });
      await until(
        () =>
          h.messages.filter(m => m.message.details.id === chatty.id).length ===
          2,
      );
      const chattyEvents = h.messages
        .filter(m => m.message.details.id === chatty.id)
        .map(m => m.message.details.kind);
      assert.deepEqual(chattyEvents, ['completed', 'update']);
      assert.match(
        (await h.call('worker_status', {id: chatty.id})).result,
        /first answer/,
      );

      // message adapts: steer a running worker; continue a finished one; wait for the reply
      const slow = await h.call('worker_spawn', {
        label: 'slow',
        task: 'slow:work',
      });
      assert.match(
        (await h.run('message', {to: slow.id, text: 'hurry'})).content[0].text,
        /steered/,
      );
      const continued = await h.run('message', {
        to: spawned.id,
        text: 'and again',
        wait: true,
        timeoutMs: 2000,
      });
      assert.match(
        continued.content[0].text,
        /completed[\s\S]*done: and again/,
      );

      // worker_wait returns the settled worker, or times out without harm
      assert.equal(
        (await h.call('worker_wait', {id: spawned.id, timeoutMs: 500})).state,
        'completed',
      );
      const pending = await h.call('worker_spawn', {
        label: 'pending',
        task: 'slow:later',
      });
      assert.match(
        (await h.run('worker_wait', {id: pending.id, timeoutMs: 100}))
          .content[0].text,
        /No change within the wait window/,
      );
      const seenBefore = h.messages.length;
      const waited = await h.run('worker_wait', {
        id: pending.id,
        timeoutMs: 2000,
      });
      assert.match(
        waited.content[0].text,
        /completed[\s\S]*slow done/,
        'wait returns the full result',
      );
      await sleep(150);
      assert.equal(
        h.messages.length,
        seenBefore,
        'the tool result consumed this completion; no second model message',
      );

      // Keep completions pending until the wait; either child may finish while
      // the other is still starting, before both spawn calls return.
      h.handlers.before_agent_start(
        {messageType: 'user', systemPromptOptions: {skills: []}},
        h.ctx,
      );
      // all: true collects every live worker and its pending completion.
      const [p1, p2] = await Promise.all([
        h.call('worker_spawn', {label: 'p1', task: 'slow:one'}),
        h.call('worker_spawn', {label: 'p2', task: 'slow:two'}),
      ]);
      const everyone = await h.run('worker_wait', {all: true, timeoutMs: 5000});
      assert.match(
        everyone.content[0].text,
        new RegExp(`${p1.id} p1 completed[\\s\\S]*slow done`),
      );
      assert.match(
        everyone.content[0].text,
        new RegExp(`${p2.id} p2 completed[\\s\\S]*slow done`),
      );

      // all:true includes already-settled completions only when it returns
      // their text, both with another worker live and when nothing remains live.
      h.handlers.before_agent_start(
        {messageType: 'user', systemPromptOptions: {skills: []}},
        h.ctx,
      );
      const queuedComplete = await h.call('worker_spawn', {
        label: 'queued',
        task: 'quick',
      });
      const stillWorking = await h.call('worker_spawn', {
        label: 'still-working',
        task: 'slow:waiting',
      });
      await until(
        () =>
          h.entries.findLast(
            entry =>
              entry.customType === 'dots.worker' &&
              entry.data.id === queuedComplete.id,
          )?.data.state === 'completed',
      );
      const beforeQueued = h.messages.length;
      const collected = await h.run('worker_wait', {
        all: true,
        timeoutMs: 2000,
      });
      assert.match(
        collected.content[0].text,
        new RegExp(`${queuedComplete.id} queued completed[\\s\\S]*done: quick`),
      );
      assert.match(
        collected.content[0].text,
        new RegExp(
          `${stillWorking.id} still-working completed[\\s\\S]*slow done`,
        ),
      );
      assert.equal(
        await h.handlers.agent_before_settle(
          {
            outcome: 'completed',
            entries: [],
            continue: false,
            context: {canContinue: false},
          },
          h.ctx,
        ),
        undefined,
      );
      h.handlers.agent_settled({aborted: false}, h.ctx);
      await sleep(150);
      assert.equal(
        h.messages.length,
        beforeQueued,
        'all:true already returned both completions to the model',
      );

      h.handlers.before_agent_start(
        {messageType: 'user', systemPromptOptions: {skills: []}},
        h.ctx,
      );
      const onlyDone = await h.call('worker_spawn', {
        label: 'only-done',
        task: 'quick',
      });
      await until(
        () =>
          h.entries.findLast(
            entry =>
              entry.customType === 'dots.worker' &&
              entry.data.id === onlyDone.id,
          )?.data.state === 'completed',
      );
      const beforeOnlyDone = h.messages.length;
      assert.match(
        (await h.run('worker_wait', {all: true})).content[0].text,
        new RegExp(`${onlyDone.id} only-done completed[\\s\\S]*done: quick`),
      );
      h.handlers.agent_settled({aborted: false}, h.ctx);
      await sleep(150);
      assert.equal(
        h.messages.length,
        beforeOnlyDone,
        'all:true consumes the queued result even with no live worker',
      );

      // The parent can be between turns while two children finish: one boundary
      // draft contains frozen outcomes, and the draft itself makes continuation valid.
      h.handlers.before_agent_start(
        {messageType: 'user', systemPromptOptions: {skills: []}},
        h.ctx,
      );
      const [batchA, batchB] = await Promise.all([
        h.call('worker_spawn', {label: 'batch-a', task: 'slow:batch-a'}),
        h.call('worker_spawn', {label: 'batch-b', task: 'slow:batch-b'}),
      ]);
      const beforeBatch = h.messages.length;
      await until(() =>
        [batchA, batchB].every(
          w =>
            h.entries.findLast(
              entry =>
                entry.customType === 'dots.worker' && entry.data.id === w.id,
            )?.data.state === 'completed',
        ),
      );
      assert.equal(
        h.messages.length,
        beforeBatch,
        'busy owner does not receive stray automatic turns',
      );
      const boundary = await h.handlers.agent_before_settle(
        {
          outcome: 'completed',
          entries: [],
          continue: false,
          context: {canContinue: false},
        },
        h.ctx,
      );
      assert.equal(boundary.continue, true);
      assert.equal(boundary.entries.length, 1);
      assert.deepEqual(
        boundary.entries[0].details.deliveryIds.sort(),
        [`${batchA.id}:1`, `${batchB.id}:1`].sort(),
      );
      assert.match(boundary.entries[0].content, /batch-a/);
      assert.match(boundary.entries[0].content, /batch-b/);
      h.entries.push(boundary.entries[0]);
      h.handlers.message_end({
        message: {role: 'custom', ...boundary.entries[0]},
      });
      h.handlers.agent_settled({aborted: false}, h.ctx);
      assert.equal(h.messages.length, beforeBatch);

      // Aborting the parent suppresses autonomous model turns but records the
      // next child's completion for the next explicit user prompt.
      h.handlers.before_agent_start(
        {messageType: 'user', systemPromptOptions: {skills: []}},
        h.ctx,
      );
      h.handlers.agent_settled({aborted: true}, h.ctx);
      const afterAbort = await h.call('worker_spawn', {
        label: 'after-abort',
        task: 'quick',
      });
      await until(() =>
        h.messages.some(
          message => message.message.details.id === afterAbort.id,
        ),
      );
      assert.equal(
        h.messages.findLast(
          message => message.message.details.id === afterAbort.id,
        ).options.triggerTurn,
        false,
      );
      h.handlers.before_agent_start(
        {messageType: 'user', systemPromptOptions: {skills: []}},
        h.ctx,
      );
      h.handlers.agent_settled({aborted: false}, h.ctx);

      // reload: processes are gone, registry says paused, message continues on --session
      const running = await h.call('worker_spawn', {
        label: 'long',
        task: 'slow:forever',
      });
      assert.ok(intervals.size >= 1);
      const queuedTick = [...intervals.values()][0];
      const outstandingWait = h.run('worker_wait', {id: running.id});
      const shutdown = h.handlers.session_shutdown({}, h.ctx);
      assert.equal(
        intervals.size,
        0,
        'shutdown clears the ticker before awaiting children',
      );
      await shutdown;
      await outstandingWait;
      stale = true;
      const rendersBefore = renders;
      assert.doesNotThrow(
        () => queuedTick(),
        'even a queued callback cannot use a stale ctx',
      );
      assert.equal(
        renders,
        rendersBefore,
        'shutdown releases the render callback',
      );
      const reloaded = harness(root);
      harnesses.push(reloaded);
      reloaded.entries.push(...h.entries);
      reloaded.handlers.session_start({reason: 'reload'}, reloaded.ctx);
      assert.equal(
        (await reloaded.call('worker_status', {id: running.id})).state,
        'paused',
      );
      assert.match(
        (await reloaded.run('message', {to: running.id, text: 'pick up'}))
          .content[0].text,
        /continued in its own session/,
      );
      await until(() => reloaded.messages.length === 1);
      assert.match(reloaded.messages[0].message.content, /done: pick up/);

      // a continued worker keeps its model, tools, and contract
      const argsFile = path.join(tmpdir(), `dots-fake-pi-args-${running.id}`);
      const continuedArgs = JSON.parse(readFileSync(argsFile, 'utf8'));
      assert.ok(
        continuedArgs.includes('--session'),
        'continued on its session',
      );
      assert.ok(
        continuedArgs.includes('--append-system-prompt'),
        'contract re-applied',
      );
      rmSync(argsFile, {force: true});
      const scoped = await reloaded.call('worker_spawn', {
        label: 'scoped',
        task: 'quick',
        tools: ['read'],
        model: 'fake/model',
        thinking: 'low',
      });
      await until(() => reloaded.messages.length === 2);
      await reloaded.handlers.session_shutdown({}, reloaded.ctx);
      const third = harness(root);
      harnesses.push(third);
      third.entries.push(...reloaded.entries);
      third.handlers.session_start({reason: 'reload'}, third.ctx);
      await third.run('message', {to: scoped.id, text: 'more'});
      const scopedArgs = JSON.parse(
        readFileSync(
          path.join(tmpdir(), `dots-fake-pi-args-${scoped.id}`),
          'utf8',
        ),
      );
      assert.ok(
        scopedArgs.includes('--tools') &&
          scopedArgs[scopedArgs.indexOf('--tools') + 1] === 'read,message',
      );
      assert.ok(
        scopedArgs.includes('--model') &&
          scopedArgs[scopedArgs.indexOf('--model') + 1] === 'fake/model',
      );
      assert.ok(
        scopedArgs.includes('--thinking') &&
          scopedArgs[scopedArgs.indexOf('--thinking') + 1] === 'low',
      );
      assert.ok(
        scopedArgs.includes('--no-mcp'),
        'a tool list without codemode or tool_search cannot reach MCP, so it is not connected',
      );
      rmSync(path.join(tmpdir(), `dots-fake-pi-args-${scoped.id}`), {
        force: true,
      });

      const emptyTools = await third.call('worker_spawn', {
        label: 'empty-tools',
        task: 'quick',
        tools: [],
      });
      const emptyArgsFile = path.join(
        tmpdir(),
        `dots-fake-pi-args-${emptyTools.id}`,
      );
      const emptyArgs = JSON.parse(readFileSync(emptyArgsFile, 'utf8'));
      assert.equal(
        emptyArgs[emptyArgs.indexOf('--tools') + 1],
        'message',
        'empty allowlist is not the default tool set',
      );
      rmSync(emptyArgsFile, {force: true});

      // confirm dialogs are answered on the protocol's field
      const confirmer = await third.call('worker_spawn', {
        label: 'confirmer',
        task: 'confirm:Proceed?',
      });
      await until(() =>
        third.messages.some(
          m =>
            m.message.details.id === confirmer.id &&
            m.message.details.kind === 'asking',
        ),
      );
      assert.match(
        (await third.run('worker_wait', {all: true})).content[0].text,
        /asking.*answer it/,
      );
      await third.run('message', {to: confirmer.id, text: 'yes'});
      await until(() =>
        third.messages.some(m => /confirmed=true/.test(m.message.content)),
      );

      // a child that fails to start is recorded as failed, not left running
      await assert.rejects(
        third.run('worker_spawn', {
          label: 'doomed',
          task: 'x',
          model: 'broken/model',
        }),
        /failed to start/,
      );
      const ghost = (await third.call('worker_status', {})).find(
        w => w.label === 'doomed',
      );
      assert.equal(ghost.state, 'failed');
      assert.match(ghost.error, /Model not found/);

      // stop ends the process; the session file remains addressable; bad addresses are refused
      const victim = await third.call('worker_spawn', {
        label: 'victim',
        task: 'slow:ignore-abort',
      });
      const stoppedResult = await third.run('worker_stop', {id: victim.id});
      const stopped = stoppedResult.details;
      assert.match(
        stoppedResult.content[0].text,
        /Saved session retained.*message can continue/,
      );
      assert.equal(stopped.state, 'stopped');
      assert.equal(stopped.processState, 'closed');
      assert.ok(stopped.session);
      await assert.rejects(
        third.run('message', {to: 'parent', text: 'hi'}),
        /no parent/,
      );
      await assert.rejects(
        third.run('worker_stop', {id: ''}),
        /required|needs id/,
      );
      await assert.rejects(third.run('worker_stop', {id: 'w-'}), /ambiguous/);

      // stop all: every live worker of this session, finished ones untouched
      const [s1, s2] = await Promise.all([
        third.call('worker_spawn', {label: 'slow-1', task: 'slow:a'}),
        third.call('worker_spawn', {label: 'slow-2', task: 'slow:b'}),
      ]);
      const all = await third.call('worker_stop', {all: true});
      assert.deepEqual(
        all.stopped.map(w => w.id).sort(),
        [s1.id, s2.id].sort(),
      );
      assert.ok(all.stopped.every(w => w.state === 'stopped'));
      assert.equal(
        (await third.call('worker_stop', {all: true})).stopped.length,
        0,
      );
      assert.equal(
        (await third.call('worker_status', {})).find(w => w.label === 'victim')
          .state,
        'stopped',
      );

      // A waiting delivery must not consume the previous run's in-flight settle.
      const prior = await third.call('worker_spawn', {
        label: 'prior',
        task: 'delay-result',
      });
      await until(() => existsSync(`${prior.session}.reading`));
      const fresh = await third.run('message', {
        to: prior.id,
        text: 'new result',
        wait: true,
        timeoutMs: 2000,
      });
      assert.match(fresh.content[0].text, /done: new result/);
      assert.doesNotMatch(fresh.content[0].text, /old result/);

      const questions = await third.call('worker_spawn', {
        label: 'questions',
        task: 'two-questions',
      });
      await until(
        () =>
          third.messages.filter(
            m =>
              m.message.details.id === questions.id &&
              m.message.details.kind === 'asking',
          ).length === 2,
      );
      assert.equal(
        (await third.call('worker_status', {id: questions.id})).pendingQuestions
          .length,
        2,
      );
      const nextQuestion = await third.run('message', {
        to: questions.id,
        text: 'one',
        wait: true,
        timeoutMs: 2000,
      });
      assert.match(nextQuestion.content[0].text, /second question/);
      assert.equal(
        (await third.call('worker_status', {id: questions.id})).question,
        'second question',
      );
      await third.call('message', {
        to: questions.id,
        text: 'two',
        wait: true,
        timeoutMs: 2000,
      });
      assert.equal(
        (await third.call('worker_status', {id: questions.id})).result,
        'both answered',
      );
      const expired = await third.call('worker_spawn', {
        label: 'expired',
        task: 'expired-question',
      });
      await sleep(80);
      const afterExpiry = await third.run('message', {
        to: expired.id,
        text: 'after expiry',
        wait: true,
        timeoutMs: 2000,
      });
      assert.match(afterExpiry.content[0].text, /steered: after expiry/);

      const crashed = await third.call('worker_spawn', {
        label: 'crashed',
        task: 'crash-question',
      });
      await until(() =>
        third.messages.some(
          m =>
            m.message.details.id === crashed.id &&
            m.message.details.kind === 'failed',
        ),
      );
      assert.equal(
        (await third.call('worker_status', {id: crashed.id})).question,
        undefined,
      );
      const resumed = await third.run('message', {
        to: crashed.id,
        text: 'recover after crash',
        wait: true,
        timeoutMs: 2000,
      });
      assert.match(resumed.content[0].text, /done: recover after crash/);

      // Accepted prompts can fail at the provider; no final text is not success.
      const provider = await third.call('worker_spawn', {
        label: 'provider',
        task: 'provider-error',
      });
      await until(() =>
        third.messages.some(m => m.message.details.id === provider.id),
      );
      assert.equal(
        (await third.call('worker_status', {id: provider.id})).state,
        'failed',
      );
      assert.match(
        (await third.run('worker_status', {id: provider.id})).content[0].text,
        /provider unavailable/,
      );
      const interrupted = await third.call('worker_spawn', {
        label: 'interrupted',
        task: 'aborted:partial',
      });
      await until(
        () =>
          third.entries.findLast(
            entry =>
              entry.customType === 'dots.worker' &&
              entry.data.id === interrupted.id,
          )?.data.state === 'interrupted',
      );
      const partial = await third.call('worker_status', {id: interrupted.id});
      assert.equal(partial.result, undefined);
      assert.equal(partial.partial, 'partial answer');
      assert.match(
        (await third.run('worker_status', {id: interrupted.id})).content[0]
          .text,
        /partial response \(not final\)/,
      );
      const missing = await third.call('worker_spawn', {
        label: 'missing',
        task: 'quick',
      });
      await until(
        () =>
          third.entries.findLast(
            entry =>
              entry.customType === 'dots.worker' &&
              entry.data.id === missing.id,
          )?.data.state === 'completed',
      );
      const unknown = await third.run('message', {
        to: missing.id,
        text: 'missing:final',
        wait: true,
        timeoutMs: 2000,
      });
      assert.equal(
        unknown.details.state,
        'unknown',
        "the prior run's final text cannot satisfy a missing response",
      );
      assert.equal(unknown.details.result, undefined);
      assert.match(
        unknown.content[0].text,
        /No assistant response was recorded/,
      );

      // Failed or handled continuations retire the process, remain resumable,
      // and clear their error when a later explicit retry succeeds.
      for (const text of ['reject', 'handled']) {
        await assert.rejects(
          third.run('message', {to: provider.id, text}),
          /rejected continuation|without starting/,
        );
        assert.equal(
          (await third.call('worker_status', {id: provider.id})).state,
          'failed',
        );
        await third.run('message', {
          to: provider.id,
          text: 'recover',
          wait: true,
          timeoutMs: 2000,
        });
        const recovered = await third.call('worker_status', {id: provider.id});
        assert.equal(recovered.state, 'completed');
        assert.equal(recovered.error, undefined);
      }

      // Two continuations of a paused child serialize and share one resumed run.
      await third.call('worker_stop', {id: provider.id});
      await Promise.all([
        third.run('message', {to: provider.id, text: 'delayed-ack'}),
        third.run('message', {to: provider.id, text: 'second'}),
      ]);
      await third.run('worker_wait', {id: provider.id, timeoutMs: 2000});
      assert.match(
        (await third.call('worker_status', {id: provider.id})).result,
        /second/,
      );

      // Stop preempts an unacknowledged message and invalidates queued messages.
      const blocked = assert.rejects(
        third.run('message', {to: provider.id, text: 'no-ack'}),
        /closing|cancelled/,
      );
      const queued = assert.rejects(
        third.run('message', {to: provider.id, text: 'must not resume'}),
        /cancelled/,
      );
      await until(
        () =>
          third.entries.findLast(entry => entry.data.id === provider.id)?.data
            .state === 'running',
      );
      await third.call('worker_stop', {id: provider.id});
      await Promise.all([blocked, queued]);
      assert.equal(
        (await third.call('worker_status', {id: provider.id})).state,
        'stopped',
      );

      const idleCleanup = await third.run('worker_stop', {all: true});
      assert.equal(
        idleCleanup.details.stopped.length,
        0,
        'there was no active work',
      );
      assert.ok(
        idleCleanup.details.closed.includes(questions.id),
        'retained idle processes were still closed',
      );
      assert.match(idleCleanup.content[0].text, /Closed \d+ owned processes/);
      assert.match(
        idleCleanup.content[0].text,
        /Finished results and saved sessions are retained/,
      );
      assert.equal(
        (await third.call('worker_status', {id: questions.id})).state,
        'completed',
      );
      assert.equal(
        (await third.call('worker_status', {id: questions.id})).processState,
        'closed',
      );
      assert.match(
        (await third.run('worker_status', {id: questions.id})).content[0].text,
        /process closed/,
      );
      assert.match(
        (await third.run('worker_stop', {all: true})).content[0].text,
        /No owned processes needed closing/,
      );

      // Completed owners/intermediates stay visible while descendants are live.
      const statuses = [];
      third.ctx.mode = 'tui';
      third.ctx.hasUI = true;
      third.ctx.ui = {
        setStatus(key, text) {
          statuses.push([key, text]);
        },
        setWidget() {},
        notify() {},
      };
      const owner = await third.call('worker_spawn', {
        label: 'owner',
        task: 'slow:owner',
      });
      const registry = openRegistry(
        path.join(root, 'agent/dots/workers.db'),
        m => assert.fail(m),
      );
      try {
        const row = (id, parent, state) => ({
          id,
          parent,
          root: owner.id,
          owner_session: null,
          label: id,
          state,
          cwd: root,
          task_project: null,
          task_id: null,
          model: null,
          thinking: null,
          activity: null,
          failed: null,
          question: state === 'asking' ? 'Which option?' : null,
          calls: 0,
          tokens: 0,
          started_at: Date.now(),
          last_event_at: Date.now(),
          updated_at: Date.now(),
          session_file: null,
          pid: process.pid,
        });
        registry.upsert(row('w-middle', owner.id, 'completed'));
        registry.upsert(row('w-leaf', 'w-middle', 'asking'));
        registry.upsert(row('w-unrelated', null, 'running'));
        await third.run('worker_wait', {id: owner.id, timeoutMs: 2000});
        assert.deepEqual(
          statuses,
          [],
          'worker activity does not publish a redundant footer count',
        );
        // The owner finished, but its descendant still works: it stays active for the status owner.
        const active = third.published.at(-1).items;
        assert.deepEqual(
          active.map(item => [item.id, item.message]),
          [[`worker/${owner.id}`, 'waiting on 1 worker']],
        );
        assert.equal(
          intervals.size,
          1,
          'descendants keep the observation tick alive',
        );
        assert.match(
          (await third.run('worker_status', {id: 'w-leaf'})).content[0].text,
          /Which option/,
        );
        await assert.rejects(
          third.run('worker_status', {id: 'w-unrelated'}),
          /no worker/,
        );
        const stoppedTree = await third.call('worker_stop', {all: true});
        assert.ok(stoppedTree.stopped.some(w => w.id === owner.id));
        assert.equal(
          (await third.call('worker_status', {id: owner.id})).state,
          'completed',
          'closing retained owners preserves completed results',
        );
        registry.upsert(row('w-leaf', 'w-middle', 'paused'));
        // Nothing live remains; recently finished workers age out of the widget, then the ticker stops.
        const realNow = Date.now;
        Date.now = () => realNow() + 121_000;
        try {
          for (const tick of [...intervals.values()]) tick();
        } finally {
          Date.now = realNow;
        }
        assert.equal(intervals.size, 0);
      } finally {
        registry.close();
      }

      // A command already stopping children must not notify through a retired UI.
      await third.call('worker_spawn', {
        label: 'closing',
        task: 'slow:closing',
      });
      const stoppingCommand = third.commands
        .get('workers')
        .handler('stop', third.ctx);
      const shutdownDuringStop = third.handlers.session_shutdown({}, third.ctx);
      third.ctx.ui.notify = () =>
        assert.fail('old command UI used after shutdown');
      await Promise.all([stoppingCommand, shutdownDuringStop]);

      // A terminal worker line calls interrupted text partial, not a final result.
      const preview = harness(root);
      harnesses.push(preview);
      preview.ctx.hasUI = true;
      preview.ctx.mode = 'tui';
      let widgetFactory;
      preview.ctx.ui = {
        setStatus() {},
        setWidget(_key, factory) {
          if (factory) widgetFactory = factory;
        },
        notify() {},
      };
      await preview.handlers.session_start({}, preview.ctx);
      const partialWidget = await preview.call('worker_spawn', {
        label: 'partial-widget',
        task: 'aborted:partial',
      });
      await until(
        () =>
          preview.entries.findLast(
            entry =>
              entry.customType === 'dots.worker' &&
              entry.data.id === partialWidget.id,
          )?.data.state === 'interrupted',
      );
      assert.ok(widgetFactory);
      const plainTheme = {fg: (_tone, text) => text, bold: text => text};
      const widgetText = widgetFactory({requestRender() {}}, plainTheme)
        .render(120)
        .join('\n');
      assert.match(
        widgetText,
        /partial-widget.*interrupted.*partial: partial answer/,
      );
      const headlineWorker = await preview.call('worker_spawn', {
        label: 'headline-worker',
        task: 'headline:stream',
      });
      await preview.call('worker_wait', {
        id: headlineWorker.id,
        timeoutMs: 2000,
      });
      assert.equal(
        (await preview.call('worker_status', {id: headlineWorker.id})).headline,
        'Inspecting API',
        'streamed reasoning supplies a fallback',
      );
      for (const [task, expected] of [
        ['headline:final', 'Final reasoning'],
        ['headline:progress', 'Validation complete'],
        ['headline:final', 'Final reasoning'],
      ]) {
        await preview.call('message', {to: headlineWorker.id, text: task});
        await preview.call('worker_wait', {
          id: headlineWorker.id,
          timeoutMs: 2000,
        });
        assert.equal(
          (await preview.call('worker_status', {id: headlineWorker.id}))
            .headline,
          expected,
          'progress overrides later streamed/final reasoning; continuation resets precedence',
        );
      }
      await preview.call('message', {
        to: headlineWorker.id,
        text: 'headline:long-note',
      });
      await preview.call('worker_wait', {
        id: headlineWorker.id,
        timeoutMs: 2000,
      });
      const longNote = preview.messages.findLast(
        entry => entry.message.details.kind === 'note',
      );
      assert.match(
        longNote.message.content,
        /Note preview limited; inspect worker_status/,
      );
      assert.equal(longNote.message.display, false);
      assert.equal(longNote.options.triggerTurn, false);
      await preview.handlers.session_shutdown({}, preview.ctx);
    } finally {
      // Always end child processes, or a failed assertion leaves pipes holding the test open.
      for (const h of harnesses) await h.handlers.session_shutdown({}, h.ctx);
      const ids = new Set(
        harnesses.flatMap(h =>
          h.entries
            .filter(
              entry =>
                entry.type === 'custom' && entry.customType === 'dots.worker',
            )
            .map(entry => entry.data.id),
        ),
      );
      for (const id of ids)
        rmSync(path.join(tmpdir(), `dots-fake-pi-args-${id}`), {force: true});
      for (const key of Object.keys(process.env))
        if (!(key in saved)) delete process.env[key];
      Object.assign(process.env, saved);
      rmSync(root, {recursive: true, force: true});
    }
  });
}

if (process.env.DOTS_WORKERS_FIXTURE !== 'pi') {
  test(
    'workers are session resources: records persist an allowlist, forks inherit history, and bad records are diagnosed',
    {timeout: 30_000},
    async t => {
      const root = mkdtempSync(path.join(tmpdir(), 'dots-workers-owner-'));
      const saved = {...process.env};
      process.env.DOTS_WORKERS_FIXTURE = 'pi';
      process.env.PI_CODING_AGENT_DIR = path.join(root, 'agent');
      delete process.env.DOTS_WORKER_ID;
      delete process.env.DOTS_WORKER_DEPTH;
      const harnesses = [];
      const records = (h, id) =>
        h.entries
          .filter(
            entry =>
              entry.type === 'custom' &&
              entry.customType === 'dots.worker' &&
              entry.data.id === id,
          )
          .map(entry => entry.data);
      try {
        const owner = harness(root, 'session-owner');
        harnesses.push(owner);
        const warnings = [];
        owner.ctx.hasUI = true;
        owner.ctx.ui = {
          notify: message => warnings.push(message),
          setStatus() {},
          setWidget() {},
        };
        await owner.handlers.session_start({reason: 'startup'}, owner.ctx);
        const brief = `a long brief that must not be copied into every record ${'x'.repeat(2000)}`;
        const done = await owner.call('worker_spawn', {
          label: 'owned',
          task: brief,
        });
        await until(
          () => records(owner, done.id).at(-1)?.state === 'completed',
        );
        const persisted = records(owner, done.id);
        assert.ok(persisted.length >= 2);
        const {WorkerRecordSchema, decode} =
          await import('../../extensions/workers/record.ts');
        for (const record of persisted)
          assert.equal(
            decode(WorkerRecordSchema, record).ok,
            true,
            'every write decodes on restore',
          );
        for (const record of persisted) {
          assert.equal(record.version, 1);
          assert.equal(record.owner, 'session-owner');
          for (const runtime of [
            'task',
            'process',
            'generation',
            'settling',
            'lastResponse',
            'question',
            'tool',
            'activity',
            'headline',
            'phase',
            'lastEventAt',
          ]) {
            assert.equal(
              Object.hasOwn(record, runtime),
              false,
              `${runtime} is runtime state, not persisted`,
            );
          }
          // The scripted child echoes its brief as the result; before then no record carries it.
          if (record.state === 'running')
            assert.doesNotMatch(
              JSON.stringify(record),
              /a long brief/,
              "the brief lives in the child's own session, not in each record",
            );
        }
        // Lock contention skips one observation; the registry stays enabled.
        const busy = Object.assign(new Error('database is locked'), {
          code: 'ERR_SQLITE_ERROR',
          errcode: 5,
        });
        t.mock.method(
          Registry.prototype,
          'events',
          () => {
            throw busy;
          },
          {times: 1},
        );
        const contended = await owner.run('worker_status', {id: done.id});
        assert.doesNotMatch(
          contended.content[0].text,
          /activity \(oldest first\)/,
          'the contended read is skipped',
        );
        assert.ok(
          Array.isArray(contended.details.recent),
          'and the next read in the same call succeeds',
        );
        assert.deepEqual(
          warnings.filter(text => /registry/.test(text)),
          [],
          'contention is not reported as a failure',
        );
        t.mock.restoreAll();
        await owner.handlers.session_shutdown({reason: 'fork'}, owner.ctx);

        // /fork and /clone copy entries into a new session id: the parent's workers are read-only history there.
        const fork = harness(root, 'session-fork');
        harnesses.push(fork);
        fork.entries.push(...owner.entries);
        await fork.handlers.session_start({reason: 'fork'}, fork.ctx);
        const inherited = await fork.call('worker_status', {id: done.id});
        assert.equal(inherited.owned, false);
        assert.equal(inherited.ownerSession, 'session-owner');
        assert.equal(inherited.state, 'completed');
        assert.match(
          (await fork.run('worker_status', {})).content[0].text,
          /read-only: owned by session session-owner/,
        );
        await assert.rejects(
          fork.run('message', {to: done.id, text: 'continue'}),
          /belongs to session session-owner.*read-only history/,
        );
        await assert.rejects(
          fork.run('worker_stop', {id: done.id}),
          /belongs to session session-owner/,
        );
        await assert.rejects(
          fork.run('worker_wait', {id: done.id}),
          /belongs to session session-owner/,
        );
        assert.match(
          (await fork.run('worker_wait', {})).content[0].text,
          /No workers in this session/,
        );
        assert.match(
          (await fork.run('worker_stop', {all: true})).content[0].text,
          /No owned processes needed closing/,
        );
        assert.equal(
          fork.entries.length,
          owner.entries.length,
          'the fork appends nothing for inherited workers',
        );
        assert.equal(
          fork.messages.length,
          0,
          'an inherited completion is never delivered to the fork',
        );
        // The inspector shows inherited workers read-only, and its source refuses control even if a stale view offers it.
        let shown;
        let source;
        fork.ctx.hasUI = true;
        fork.ctx.mode = 'tui';
        fork.ctx.ui = {
          notify() {},
          setStatus() {},
          setWidget() {},
          custom: async factory => {
            const view = factory(
              {requestRender() {}, terminal: {rows: 32}},
              {fg: (_tone, text) => text, bold: text => text},
              {},
              () => {},
            );
            shown = view.snapshot?.worker;
            source = view.source;
          },
        };
        await fork.commands.get('workers').handler(done.id, fork.ctx);
        assert.deepEqual(
          [shown.canMessage, shown.canStop, shown.owner],
          [false, false, 'session session-owner'],
        );
        await assert.rejects(
          source.message(done.id, 'sneak'),
          /belongs to session session-owner/,
        );
        await assert.rejects(
          source.stop(done.id),
          /belongs to session session-owner/,
        );
        await fork.handlers.session_shutdown({reason: 'quit'}, fork.ctx);

        // Resuming the original session keeps ownership and can continue the worker.
        const resumed = harness(root, 'session-owner');
        harnesses.push(resumed);
        resumed.entries.push(...owner.entries);
        await resumed.handlers.session_start({reason: 'resume'}, resumed.ctx);
        assert.equal(
          (await resumed.call('worker_status', {id: done.id})).owned,
          true,
        );
        assert.match(
          (
            await resumed.run('message', {
              to: done.id,
              text: 'pick up',
              wait: true,
              timeoutMs: 5000,
            })
          ).content[0].text,
          /done: pick up/,
        );
        await resumed.handlers.session_shutdown({reason: 'quit'}, resumed.ctx);

        // Incompatible records are diagnosed once and never executed; the rest restore.
        const damaged = harness(root, 'session-owner');
        harnesses.push(damaged);
        const notices = [];
        damaged.ctx.hasUI = true;
        damaged.ctx.ui = {
          notify: message => notices.push(message),
          setStatus() {},
          setWidget() {},
        };
        damaged.entries.push(
          {
            type: 'custom',
            customType: 'dots.worker',
            data: {
              id: 'w-00000000000000aa',
              label: 'versionless',
              state: 'paused',
              task: 'old',
              cwd: root,
              sessionDir: root,
              sessionFile: path.join(root, 'old.jsonl'),
              startedAt: 1,
              updatedAt: 1,
              calls: 0,
              tokens: 0,
            },
          },
          ...resumed.entries,
          // A newer, malformed record of a good worker does not resurrect its older one.
          {
            type: 'custom',
            customType: 'dots.worker',
            data: {...records(resumed, done.id).at(-1), state: 'sleeping'},
          },
        );
        await damaged.handlers.session_start({reason: 'reload'}, damaged.ctx);
        assert.equal(notices.length, 1);
        assert.match(
          notices[0],
          /^2 worker records were incompatible and not restored \(/,
        );
        await assert.rejects(
          damaged.run('worker_status', {id: 'w-00000000000000aa'}),
          /no worker/,
        );
        await assert.rejects(
          damaged.run('worker_status', {id: done.id}),
          /no worker/,
          'the damaged worker is diagnosed, not rolled back',
        );
        await damaged.handlers.session_shutdown({reason: 'quit'}, damaged.ctx);
      } finally {
        for (const h of harnesses) await h.handlers.session_shutdown({}, h.ctx);
        process.env = saved;
        rmSync(root, {recursive: true, force: true});
      }
    },
  );

  test('a malformed nesting depth refuses to spawn instead of resetting the guard', async () => {
    const saved = {...process.env};
    const root = mkdtempSync(path.join(tmpdir(), 'dots-workers-depth-'));
    process.env.PI_CODING_AGENT_DIR = path.join(root, 'agent');
    try {
      const h = harness(root);
      await h.handlers.session_start({reason: 'startup'}, h.ctx);
      for (const value of ['deep', '-1', '1.5']) {
        process.env.DOTS_WORKER_DEPTH = value;
        await assert.rejects(
          h.run('worker_spawn', {label: 'nested', task: 'x'}),
          /DOTS_WORKER_DEPTH must be a non-negative integer/,
        );
      }
      assert.equal(
        h.entries.filter(entry => entry.customType === 'dots.worker').length,
        0,
        'nothing was launched',
      );
      const message = h.tools.get('message').parameters.properties;
      assert.deepEqual(
        [
          message.timeoutMs.type,
          message.timeoutMs.minimum,
          message.timeoutMs.maximum,
        ],
        ['integer', 1, 86_400_000],
      );
      const status = h.tools.get('worker_status').parameters.properties;
      assert.deepEqual(
        [status.tail.type, status.tail.minimum, status.tail.maximum],
        ['integer', 0, 100],
      );
      await h.handlers.session_shutdown({}, h.ctx);
    } finally {
      process.env = saved;
      rmSync(root, {recursive: true, force: true});
    }
  });
}
