import assert from 'node:assert/strict';
import test from 'node:test';
import {
  assistantResponse,
  settledOutcome,
} from '../extensions/workers/outcome.ts';
import {CompletionOutbox} from '../extensions/workers/outbox.ts';

const item = (id, text) => ({
  key: `${id}:1`,
  content: `Worker ${id} [${id}] completed\n\n${text}`,
  details: {
    id,
    label: id,
    kind: 'completed',
    tokens: 7,
    cost: 0.004,
    context: 31,
  },
});
const OWNER = 'session-owner';
// A session file as Pi keeps it: entries with ids, in file order; one branch unless a test moves the leaf.
const branch = () => {
  const entries = [];
  let next = 0;
  return {
    entries,
    pi: {
      appendEntry(customType, data) {
        entries.push({type: 'custom', id: `e${++next}`, customType, data});
      },
    },
  };
};
const bound = (outbox, entries, path = entries, owner = OWNER) => {
  outbox.restore(owner, entries, path);
  return outbox;
};

test('response truth distinguishes final, provider error, interruption, and absent evidence', () => {
  const final = assistantResponse({
    role: 'assistant',
    stopReason: 'stop',
    content: [{type: 'text', text: 'answer'}],
  });
  assert.deepEqual(settledOutcome(final, false), {
    state: 'completed',
    text: 'answer',
  });
  assert.deepEqual(settledOutcome(final, true), {
    state: 'interrupted',
    text: 'answer',
  });
  assert.deepEqual(
    settledOutcome(
      assistantResponse({
        role: 'assistant',
        stopReason: 'error',
        errorMessage: 'provider unavailable',
        content: [{type: 'text', text: 'partial'}],
      }),
      false,
    ),
    {state: 'failed', text: 'provider unavailable'},
  );
  assert.deepEqual(
    settledOutcome(
      assistantResponse({
        role: 'assistant',
        stopReason: 'length',
        content: [{type: 'text', text: 'partial'}],
      }),
      false,
    ),
    {state: 'interrupted', text: 'partial'},
  );
  assert.match(
    settledOutcome(undefined, false).text,
    /No assistant response was recorded/,
  );
  assert.equal(settledOutcome(undefined, false).state, 'unknown');
  assert.equal(
    settledOutcome(
      assistantResponse({
        role: 'assistant',
        stopReason: 'toolUse',
        content: [],
      }),
      false,
    ).state,
    'unknown',
  );
  assert.equal(assistantResponse({role: 'toolResult', content: []}), undefined);
});

test('completion batches freeze outcome data, restore uncommitted snapshots, and deduplicate receipts', () => {
  const {entries, pi} = branch();
  const delivered = [];
  const outbox = bound(
    new CompletionOutbox(
      pi,
      message => delivered.push(message),
      () => false,
    ),
    entries,
  );
  const first = item('w-one', 'First answer');
  outbox.offer(first);
  first.content = 'MUTATED AFTER ENQUEUE';
  first.details.cost = 999;
  outbox.offer(item('w-two', 'Second answer'));
  const draft = outbox.draft();
  assert.equal(draft.type, 'custom_message');
  assert.deepEqual(draft.details.deliveryIds, ['w-one:1', 'w-two:1']);
  assert.match(draft.content, /First answer[\s\S]*Second answer/);
  assert.doesNotMatch(draft.content, /MUTATED/);
  assert.equal(delivered.length, 0);
  outbox.close();
  const resumed = bound(
    new CompletionOutbox(
      pi,
      message => delivered.push(message),
      () => false,
    ),
    entries,
  );
  assert.equal(
    resumed.hasPending(),
    true,
    'draft without committed message replays after reload',
  );
  const replay = resumed.draft();
  assert.match(replay.content, /First answer[\s\S]*Second answer/);
  entries.push({
    type: 'custom_message',
    id: 'receipt',
    customType: replay.customType,
    details: replay.details,
  });
  resumed.committed({
    role: 'custom',
    customType: replay.customType,
    details: replay.details,
  });
  assert.equal(resumed.hasPending(), false);
  resumed.close();
  const again = bound(
    new CompletionOutbox(
      pi,
      () => assert.fail('already delivered'),
      () => true,
    ),
    entries,
  );
  assert.equal(again.hasPending(), false, 'branch receipt suppresses replay');
  again.close();
});

test('idle delivery exceptions retain the journaled result without crashing or retrying a timer', t => {
  const callbacks = new Map();
  t.mock.method(globalThis, 'setTimeout', callback => {
    const timer = {};
    callbacks.set(timer, callback);
    return timer;
  });
  t.mock.method(globalThis, 'clearTimeout', timer => callbacks.delete(timer));
  const {entries, pi} = branch();
  const failures = [];
  let attempts = 0;
  const outbox = bound(
    new CompletionOutbox(
      pi,
      () => {
        attempts++;
        throw new Error('fixture transport unavailable');
      },
      () => true,
      error => failures.push(error.message),
    ),
    entries,
  );
  outbox.offer(item('w-one', 'Recorded answer'));
  assert.equal(callbacks.size, 1);
  const [[timer, callback]] = callbacks;
  callbacks.delete(timer);
  assert.doesNotThrow(callback);
  assert.equal(attempts, 1);
  assert.deepEqual(failures, ['fixture transport unavailable']);
  assert.equal(callbacks.size, 0, 'the failed idle timer must not spin');
  assert.equal(
    outbox.queued('w-one:1')?.details.cost,
    0.004,
    'failure releases the claim without dropping the snapshot',
  );
  outbox.close();
  const reloaded = bound(
    new CompletionOutbox(
      pi,
      () => assert.fail('reload should wait for the hold'),
      () => false,
    ),
    entries,
  );
  assert.match(reloaded.draft().content, /Recorded answer/);
  reloaded.close();
});

test('late worker_wait consumption survives reload and prevents any model notification', () => {
  const {entries, pi} = branch();
  const outbox = bound(
    new CompletionOutbox(
      pi,
      () => assert.fail('consumed result already in tool output'),
      () => true,
    ),
    entries,
  );
  outbox.offer(item('w-one', 'Answer'));
  outbox.consume('w-one:1');
  assert.equal(outbox.draft(), undefined);
  outbox.close();
  const restored = bound(
    new CompletionOutbox(
      pi,
      () => assert.fail('already consumed'),
      () => true,
    ),
    entries,
  );
  assert.equal(restored.hasPending(), false);
  restored.close();
});

test('completions are session resources; receipts and consumption follow the branch', () => {
  const {entries, pi} = branch();
  const outbox = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries,
  );
  outbox.offer(item('w-one', 'Answer'));
  const draft = outbox.draft();
  entries.push({
    type: 'custom_message',
    id: 'receipt-a',
    customType: draft.customType,
    details: draft.details,
  });
  outbox.committed({
    role: 'custom',
    customType: draft.customType,
    details: draft.details,
  });
  outbox.offer(item('w-two', 'Waited for'));
  outbox.consume('w-two:1');
  outbox.close();
  const branchA = [...entries];
  // Reload on branch A: delivered and consumed there, nothing pending.
  assert.equal(
    bound(
      new CompletionOutbox(
        pi,
        () => {},
        () => false,
      ),
      entries,
      branchA,
    ).hasPending(),
    false,
  );
  // /tree to branch B, which forked before either result: the journal is the session's,
  // the receipts are A's. Both completions are offered once more, marked with their origin.
  const branchB = [];
  const tree = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries,
    branchB,
  );
  const reoffered = tree.draft();
  assert.deepEqual(reoffered.details.deliveryIds, ['w-one:1', 'w-two:1']);
  assert.match(reoffered.content, /^Workers completed \(2\)/);
  assert.match(
    reoffered.content,
    /Worker w-one \[w-one\] completed · from another branch of this session/,
  );
  branchB.push({
    type: 'custom_message',
    id: 'receipt-b',
    customType: reoffered.customType,
    details: reoffered.details,
  });
  tree.close();
  // Reload on B is equivalent to staying there: B's receipt suppresses a third offer.
  assert.equal(
    bound(
      new CompletionOutbox(
        pi,
        () => {},
        () => false,
      ),
      entries,
      branchB,
    ).hasPending(),
    false,
  );
  const single = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries.slice(0, 1),
    [],
  );
  const one = single.draft();
  assert.equal(one.details.origin, 'other-branch');
  assert.match(
    one.content.split('\n')[0],
    /completed · from another branch of this session$/,
  );
});

test("a fork's inherited completions are never delivered and malformed journals are diagnosed", () => {
  const {entries, pi} = branch();
  const outbox = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries,
  );
  outbox.offer(item('w-one', "Parent's answer"));
  outbox.close();
  const forked = new CompletionOutbox(
    pi,
    () => assert.fail('inherited'),
    () => true,
  );
  assert.equal(forked.restore('session-fork', entries, entries), undefined);
  assert.equal(
    forked.hasPending(),
    false,
    "the forked session does not own its parent's workers",
  );
  forked.close();
  entries.push({
    type: 'custom',
    id: 'legacy',
    customType: 'dots.worker.outbox',
    data: {action: 'add', item: item('w-old', 'no version')},
  });
  entries.push({
    type: 'custom',
    id: 'broken',
    customType: 'dots.worker.outbox',
    data: {version: 1, owner: OWNER, action: 'add', item: {key: 7}},
  });
  const reloaded = new CompletionOutbox(
    pi,
    () => {},
    () => false,
  );
  const problem = reloaded.restore(OWNER, entries, entries);
  assert.match(
    problem,
    /^2 worker completion records were incompatible and not restored/,
  );
  assert.deepEqual(
    reloaded.draft().details.deliveryIds,
    ['w-one:1'],
    'valid records survive their malformed neighbours',
  );
  reloaded.close();
  assert.throws(
    () =>
      new CompletionOutbox(
        pi,
        () => {},
        () => false,
      ).offer(item('w-x', 'x')),
    /no owning session/,
  );
});

test('an unreadable latest journal record invalidates its key instead of restoring older data', () => {
  const {entries, pi} = branch();
  const outbox = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries,
  );
  outbox.offer(item('w-review', 'old result'));
  outbox.offer(item('w-other', 'unrelated result'));
  outbox.close();
  entries.push({
    type: 'custom',
    id: 'broken-add',
    customType: 'dots.worker.outbox',
    data: {
      version: 1,
      owner: OWNER,
      action: 'add',
      item: {...item('w-review', 'x'), content: 42},
    },
  });
  const reloaded = new CompletionOutbox(
    pi,
    () => {},
    () => false,
  );
  assert.match(
    reloaded.restore(OWNER, entries, entries),
    /^1 worker completion record was incompatible and not restored \(\/item\/content/,
  );
  assert.deepEqual(
    reloaded.draft().details.deliveryIds,
    ['w-other:1'],
    'the stale w-review result is not redelivered',
  );
  reloaded.close();
  // A later readable add of that key is newer than the broken one and stands.
  entries.push({
    type: 'custom',
    id: 'fixed-add',
    customType: 'dots.worker.outbox',
    data: {
      version: 1,
      owner: OWNER,
      action: 'add',
      item: item('w-review', 'new result'),
    },
  });
  const fixed = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries,
  );
  assert.match(fixed.queued('w-review:1').content, /new result/);
  fixed.close();
});

test('an unreadable consumption withholds its key on its own branch only', () => {
  const {entries, pi} = branch();
  const outbox = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries,
  );
  outbox.offer(item('w-one', 'Answer'));
  outbox.close();
  const broken = {
    type: 'custom',
    id: 'broken-consumed',
    customType: 'dots.worker.outbox',
    data: {
      version: 1,
      owner: OWNER,
      action: 'consumed',
      key: 'w-one:1',
      extra: true,
    },
  };
  const here = [...entries, broken];
  const session = [...entries, broken];
  const restored = new CompletionOutbox(
    pi,
    () => {},
    () => false,
  );
  assert.match(
    restored.restore(OWNER, session, here),
    /^1 worker completion record was incompatible/,
  );
  assert.equal(
    restored.hasPending(),
    false,
    'this branch may already have seen it; do not deliver twice',
  );
  restored.close();
  // Another branch has no consumption of any kind: the session's result is still offered there.
  const elsewhere = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    session,
    entries,
  );
  assert.deepEqual(elsewhere.draft().details.deliveryIds, ['w-one:1']);
  elsewhere.close();
});

test("a waiter's completion is journaled for the session and consumed on its branch only", () => {
  const {entries, pi} = branch();
  const outbox = bound(
    new CompletionOutbox(
      pi,
      () => assert.fail('the waiter already returned it'),
      () => true,
    ),
    entries,
  );
  outbox.taken(item('w-one', 'Waited answer'));
  assert.equal(outbox.hasPending(), false);
  assert.deepEqual(
    entries.map(entry => entry.data.action),
    ['add', 'consumed'],
  );
  outbox.close();
  assert.equal(
    bound(
      new CompletionOutbox(
        pi,
        () => {},
        () => false,
      ),
      entries,
    ).hasPending(),
    false,
    'reload on the waiting branch',
  );
  const tree = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries,
    [],
  );
  assert.match(
    tree.draft().content,
    /Waited answer/,
    'another branch receives it',
  );
  tree.close();
});

test('a damaged or missing action does not let an older record of the named key stand', () => {
  for (const action of ['invalid-add', undefined, 7]) {
    const {entries, pi} = branch();
    const outbox = bound(
      new CompletionOutbox(
        pi,
        () => {},
        () => false,
      ),
      entries,
    );
    outbox.offer(item('w-review', 'old result'));
    outbox.offer(item('w-other', 'unrelated'));
    outbox.close();
    const data = {
      version: 1,
      owner: OWNER,
      item: {...item('w-review', 'x'), content: 42},
      ...(action === undefined ? {} : {action}),
    };
    entries.push({
      type: 'custom',
      id: 'damaged',
      customType: 'dots.worker.outbox',
      data,
    });
    const restored = new CompletionOutbox(
      pi,
      () => {},
      () => false,
    );
    assert.match(
      restored.restore(OWNER, entries, entries),
      /^1 worker completion record was incompatible/,
      String(action),
    );
    assert.deepEqual(
      restored.draft().details.deliveryIds,
      ['w-other:1'],
      `no stale result after action ${String(action)}`,
    );
    restored.close();
    // The session-wide invalidation holds on any branch.
    assert.deepEqual(
      bound(
        new CompletionOutbox(
          pi,
          () => {},
          () => false,
        ),
        entries,
        [],
      ).draft().details.deliveryIds,
      ['w-other:1'],
    );
  }
});

test('a damaged or missing action on a receipt withholds its key on that branch only', () => {
  for (const action of ['invalid-consumed', undefined]) {
    const {entries, pi} = branch();
    const outbox = bound(
      new CompletionOutbox(
        pi,
        () => {},
        () => false,
      ),
      entries,
    );
    outbox.offer(item('w-one', 'Answer'));
    outbox.close();
    const damaged = {
      type: 'custom',
      id: 'damaged',
      customType: 'dots.worker.outbox',
      data: {
        version: 1,
        owner: OWNER,
        key: 'w-one:1',
        ...(action === undefined ? {} : {action}),
      },
    };
    const session = [...entries, damaged];
    const here = bound(
      new CompletionOutbox(
        pi,
        () => {},
        () => false,
      ),
      session,
      session,
    );
    assert.equal(
      here.hasPending(),
      false,
      `withheld where the damaged receipt lies (${String(action)})`,
    );
    here.close();
    assert.deepEqual(
      bound(
        new CompletionOutbox(
          pi,
          () => {},
          () => false,
        ),
        session,
        entries,
      ).draft().details.deliveryIds,
      ['w-one:1'],
      'not consumed elsewhere',
    );
  }
});

test('a malformed record that names no key is diagnosed and changes nothing else', () => {
  const {entries, pi} = branch();
  const outbox = bound(
    new CompletionOutbox(
      pi,
      () => {},
      () => false,
    ),
    entries,
  );
  outbox.offer(item('w-one', 'Answer'));
  outbox.close();
  entries.push({
    type: 'custom',
    id: 'nameless',
    customType: 'dots.worker.outbox',
    data: {version: 1, owner: OWNER, action: 'add', item: 'garbled'},
  });
  const restored = new CompletionOutbox(
    pi,
    () => {},
    () => false,
  );
  assert.match(
    restored.restore(OWNER, entries, entries),
    /^1 worker completion record was incompatible/,
  );
  assert.deepEqual(restored.draft().details.deliveryIds, ['w-one:1']);
  restored.close();
});
