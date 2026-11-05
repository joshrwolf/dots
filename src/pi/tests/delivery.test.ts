import assert from 'node:assert/strict';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import type {
  ExtensionAPI,
  ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {TurnMessages, type TurnMessage} from '../src/host/delivery.ts';

/** A host that is busy until a test says otherwise, and records what was sent. */
function host(pollMs = 20) {
  const handlers = new Map<string, ((event: unknown) => void)[]>();
  const sent: {content: unknown; triggerTurn: boolean | undefined}[] = [];
  const state = {idle: true};
  const pi = {
    on: (name: string, handler: (event: unknown) => void) => {
      handlers.set(name, [...(handlers.get(name) ?? []), handler]);
    },
    sendMessage: (message: TurnMessage, options?: {triggerTurn?: boolean}) =>
      sent.push({content: message.content, triggerTurn: options?.triggerTurn}),
  } as unknown as ExtensionAPI;
  const ctx = {isIdle: () => state.idle} as unknown as ExtensionContext;
  const delivery = new TurnMessages(pi, pollMs);
  delivery.bind(ctx);
  const emit = (name: string, event: object = {}) => {
    for (const handler of handlers.get(name) ?? []) handler(event);
  };
  const message = (content: string): TurnMessage => ({
    customType: 'fixture',
    content,
    display: true,
  });
  return {delivery, sent, state, emit, message};
}

test('an idle session gets messages sent together in one run', async () => {
  const h = host();
  const delivered: string[] = [];
  h.delivery.send(
    () => h.message('a'),
    () => delivered.push('a'),
  );
  h.delivery.send(
    () => h.message('b'),
    () => delivered.push('b'),
  );
  assert.equal(h.delivery.holding(), true, 'checked after this task');
  await Promise.resolve();
  assert.deepEqual(h.sent, [
    {content: 'a', triggerTurn: false},
    {content: 'b', triggerTurn: true},
  ]);
  assert.deepEqual(delivered, ['a', 'b']);
  assert.equal(h.delivery.holding(), false);
});

test('a busy session holds messages until a run, compaction or tree navigation ends', async () => {
  for (const ended of [
    'agent_settled',
    'session_compact',
    'session_compact_failed',
    'session_tree',
  ]) {
    // A poll far beyond the test shows the event, not the fallback, delivers.
    const h = host(60_000);
    h.state.idle = false;
    h.delivery.send(() => h.message('a'));
    h.delivery.send(() => h.message('b'));
    await sleep(10);
    assert.deepEqual(h.sent, [], ended);
    // Pi reports the end before it clears its busy state.
    h.emit(ended, {aborted: false});
    h.state.idle = true;
    await sleep(10);
    assert.deepEqual(
      h.sent,
      [
        {content: 'a', triggerTurn: false},
        {content: 'b', triggerTurn: true},
      ],
      ended,
    );
    h.delivery.close();
  }
});

test('a held message is built when it is delivered', async () => {
  const h = host();
  h.state.idle = false;
  let branch = 'old';
  h.delivery.send(() => h.message(branch));
  await sleep(10);
  branch = 'new';
  h.emit('session_tree');
  h.state.idle = true;
  await sleep(10);
  assert.deepEqual(h.sent, [{content: 'new', triggerTurn: true}]);
  h.delivery.close();
});

test('a busy session without an ending event is rechecked by the fallback poll', async () => {
  const h = host(20);
  h.state.idle = false;
  h.delivery.send(() => h.message('a'));
  await sleep(50);
  assert.deepEqual(h.sent, []);
  h.state.idle = true;
  await sleep(50);
  assert.deepEqual(h.sent, [{content: 'a', triggerTurn: true}]);
  h.delivery.close();
});

test('after a cancelled run messages are recorded without a run until the user prompts', async () => {
  const h = host();
  h.state.idle = false;
  h.delivery.send(() => h.message('held'));
  h.emit('agent_settled', {aborted: true});
  h.state.idle = true;
  await sleep(10);
  assert.deepEqual(h.sent, [{content: 'held', triggerTurn: false}]);
  h.delivery.send(() => h.message('later'));
  await Promise.resolve();
  assert.deepEqual(h.sent.at(-1), {content: 'later', triggerTurn: false});
  h.emit('before_agent_start', {messageType: 'custom'});
  h.delivery.send(() => h.message('still'));
  await Promise.resolve();
  assert.deepEqual(
    h.sent.at(-1),
    {content: 'still', triggerTurn: false},
    'a custom run is not a user prompt',
  );
  h.emit('before_agent_start', {messageType: 'user'});
  h.delivery.send(() => h.message('next'));
  await Promise.resolve();
  assert.deepEqual(h.sent.at(-1), {content: 'next', triggerTurn: true});
});

test('discarded and closed messages are never sent', async () => {
  const h = host(20);
  h.state.idle = false;
  h.delivery.send(() => h.message('stale'));
  h.delivery.discard();
  assert.equal(h.delivery.holding(), false);
  h.delivery.send(() => h.message('closed'));
  h.delivery.close();
  h.state.idle = true;
  h.emit('agent_settled', {aborted: false});
  h.delivery.send(() => h.message('unbound'));
  await sleep(50);
  assert.deepEqual(h.sent, []);
});

test('a message declined when built is not sent, and the run starts on the last one sent', async () => {
  const h = host();
  h.state.idle = false;
  const delivered: string[] = [];
  h.delivery.send(
    () => h.message('kept'),
    () => delivered.push('kept'),
  );
  h.delivery.send(
    () => undefined,
    () => delivered.push('declined'),
  );
  await sleep(10);
  h.state.idle = true;
  h.emit('agent_settled', {aborted: false});
  await sleep(10);
  assert.deepEqual(h.sent, [{content: 'kept', triggerTurn: true}]);
  assert.deepEqual(delivered, ['kept']);
  assert.equal(h.delivery.holding(), false);
  h.delivery.close();
});
