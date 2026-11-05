import assert from 'node:assert/strict';
import {existsSync, readFileSync, statSync} from 'node:fs';
import {dirname} from 'node:path';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import {dotsAttention} from '../extensions/goal/attention/source.ts';
import {
  AttentionController,
  ANSWER_MESSAGE,
  deliveredAnswers,
} from '../extensions/goal/attention/controller.ts';
import {attentionContext} from '../extensions/goal/attention/context.ts';
import {attentionModel, indicator} from '../extensions/goal/attention/model.ts';
import {HumanInputs, recordChat} from '../extensions/goal/attention/chat.ts';
import {rootRecord} from '../extensions/status/model.ts';
import {TurnMessages} from '../src/host/delivery.ts';
import {
  PROJECT,
  attentionStore,
  decision,
  decisionId,
  request,
  requestId,
} from './attention-fixture.mjs';

const goalRef = {project: PROJECT, id: 't-123abc'};
const agent = {session: 'fixture-session', owner: 'pi-fixture-session-00aa'};
const listed = (requests, extra = {}) => ({
  goals: [
    {
      project: PROJECT,
      project_name: 'fixture',
      goal: 't-123abc',
      title: 'Fixture goal',
      status: 'open',
      coordinator: null,
      mandate: null,
      requests,
      ...extra,
    },
  ],
  errors: [],
});
const settle = () => new Promise(resolve => setImmediate(resolve));

test('the source reads exactly the named goals, and nothing for none', async () => {
  const calls = [];
  const source = dotsAttention(async (args, options) => {
    calls.push({args, options});
    return listed([request()]);
  });
  const signal = new AbortController().signal;
  assert.deepEqual(await source.list([], signal), {goals: [], errors: []});
  assert.deepEqual(calls, [], 'an empty scope is empty, not a fallback');
  const other = {project: `p-${'b'.repeat(32)}`, id: 't-999'};
  const result = await source.list([goalRef, other], signal);
  assert.equal(result.goals[0].requests[0].id, requestId(1));
  assert.deepEqual(calls[0].args, [
    'attention',
    'list',
    '--scope',
    'goals',
    '--goal',
    `${PROJECT}:t-123abc`,
    '--goal',
    `${other.project}:t-999`,
  ]);
});

test('the source rejects shapes the store does not print', async () => {
  const signal = new AbortController().signal;
  for (const broken of [
    listed([
      request({
        answer: {
          choice: null,
          text: 'x',
          source: 'ui',
          provenance: null,
          interpretation: null,
          at: 1,
          by: null,
        },
        state: 'answered',
      }),
    ]),
    listed([request({project: '/not/a/key'})]),
    listed([request({state: 'pending'})]),
    {goals: []},
  ]) {
    const source = dotsAttention(async () => broken);
    await assert.rejects(source.list([goalRef], signal), /unexpected shape/);
  }
});

test('answers pass through a private file that is always removed, as the bound actor', async () => {
  let seen;
  const source = dotsAttention(async (args, options) => {
    const file = args[args.indexOf('--content-file') + 1];
    seen = {
      args,
      env: options.env,
      content: JSON.parse(readFileSync(file, 'utf8')),
      mode: statSync(file).mode & 0o777,
      directory: statSync(dirname(file)).mode & 0o777,
      file,
    };
    if (args.includes('fail')) throw new Error('store refused');
    return {
      request: request({
        state: 'answered',
        answer: {
          choice: 'one',
          text: null,
          source: 'ui',
          provenance: 'pi session s · goal popup',
          interpretation: null,
          at: 2,
          by: agent.owner,
        },
      }),
    };
  });
  const signal = new AbortController().signal;
  const actor = {env: {DOTS_AGENT_OWNER: agent.owner}};
  const saved = await source.answer(
    PROJECT,
    requestId(1),
    {source: 'ui', choice: 'one', provenance: 'pi session s · goal popup'},
    actor,
    signal,
  );
  assert.equal(saved.state, 'answered');
  assert.equal(seen.env, actor.env);
  assert.deepEqual(seen.args.slice(0, 5), [
    'attention',
    'answer',
    requestId(1),
    '--project',
    PROJECT,
  ]);
  assert.deepEqual(seen.content, {
    source: 'ui',
    choice: 'one',
    provenance: 'pi session s · goal popup',
  });
  assert.equal(seen.mode, 0o600);
  assert.equal(seen.directory, 0o700);
  assert.equal(existsSync(seen.file), false);
  await assert.rejects(
    source.answer(
      PROJECT,
      'fail',
      {source: 'ui', text: 'x', provenance: 'p'},
      actor,
      signal,
    ),
    /store refused/,
  );
  assert.equal(existsSync(dirname(seen.file)), false, 'removed after failure');
});

test('the model lists needs first and names the actual need in the indicator', () => {
  const answered = request({
    id: requestId(2),
    title: 'Pick a name',
    state: 'answered',
    created_at: 10,
    answer: {
      choice: null,
      text: 'Atlas',
      source: 'ui',
      provenance: 'p',
      interpretation: null,
      at: 3,
      by: null,
    },
  });
  const model = attentionModel(
    listed([
      answered,
      request(),
      request({
        id: requestId(3),
        title: 'Approve the release',
        created_at: 300,
      }),
    ]),
    undefined,
    new Map(),
  );
  assert.deepEqual(
    model.items.map(item => item.request.id),
    [requestId(1), requestId(3), requestId(2)],
  );
  assert.equal(
    indicator(model),
    'Needs your attention: Choose the rollout scope (+1 more)',
  );
  assert.equal(
    indicator(attentionModel(listed([answered]), undefined, new Map())),
    undefined,
    'an answered request is coordinator work, not a person’s',
  );
});

function controller(store, overrides = {}) {
  const branch = overrides.branch ?? [];
  const delivered = [];
  const signals = [];
  let changes = 0;
  const ctl = new AttentionController({
    source: store.source,
    deliver: compose => {
      const message = compose();
      delivered.push(message);
      branch.push({type: 'custom_message', ...message});
    },
    branch: () => branch,
    changed: () => changes++,
    signal: signal => signals.push(signal),
  });
  ctl.bind(overrides.agent ?? agent);
  return {ctl, branch, delivered, signals, changes: () => changes};
}

test('a UI answer persists before it shows as saved and clears its draft', async () => {
  const store = attentionStore({requests: [request()]});
  const {ctl} = controller(store);
  await ctl.setScope([goalRef]);
  ctl.drafts.set(requestId(1), 'my draft');
  const saving = ctl.answer(requestId(1), {choice: 'one'});
  assert.equal(ctl.model().items[0].state.kind, 'saving');
  await saving;
  const answer = store.state.calls.find(call => call.op === 'answer');
  assert.deepEqual(answer.content, {
    source: 'ui',
    choice: 'one',
    provenance: 'pi session fixture-session · goal popup',
  });
  assert.equal(answer.actor.env.DOTS_AGENT_OWNER, agent.owner);
  assert.equal(ctl.model().items[0].state.kind, 'answered');
  assert.equal(ctl.drafts.has(requestId(1)), false);
});

test('a stale answer shows the immutable replacement and keeps the draft', async () => {
  const store = attentionStore({requests: [request()]});
  const {ctl} = controller(store);
  await ctl.setScope([goalRef]);
  store.supersede(
    requestId(1),
    request({
      id: requestId(2),
      question: 'Which projects, now that the catalog moved?',
    }),
  );
  ctl.drafts.set(requestId(1), 'only this one');
  await ctl.answer(requestId(1), {text: 'only this one'});
  const stale = ctl
    .model()
    .items.find(item => item.request.id === requestId(1));
  assert.equal(stale.state.kind, 'stale');
  assert.equal(stale.state.replacement.id, requestId(2));
  assert.equal(ctl.drafts.get(requestId(1)), 'only this one');
  const replacement = ctl
    .model()
    .items.find(item => item.request.id === requestId(2));
  assert.equal(
    replacement.state.kind,
    'open',
    'never applied to the replacement',
  );
  assert.equal(store.state.requests.get(requestId(2)).answer, null);
  ctl.dismiss(requestId(1));
  assert.equal(ctl.drafts.has(requestId(1)), false);
});

test('an operational failure leaves the request open with the error; answered elsewhere is not an error', async () => {
  const store = attentionStore({requests: [request()]});
  const {ctl} = controller(store);
  await ctl.setScope([goalRef]);
  const answer = store.source.answer;
  store.source.answer = async () => {
    throw new Error('database is locked');
  };
  await ctl.answer(requestId(1), {choice: 'one'});
  assert.deepEqual(ctl.model().items[0].state, {
    kind: 'error',
    message: 'database is locked',
  });
  store.source.answer = async (project, id, content, actor) => {
    await answer(
      project,
      id,
      {source: 'ui', text: 'elsewhere', provenance: 'p'},
      actor,
    );
    throw new Error('request is answered, not open');
  };
  await ctl.answer(requestId(1), {choice: 'one'});
  assert.equal(ctl.model().items[0].state.kind, 'answered');
});

test('only the unheld coordinator delivers, once per branch, and recovers an undelivered save', async () => {
  const answered = request({
    state: 'answered',
    answer: {
      choice: 'one',
      text: null,
      source: 'ui',
      provenance: 'pi session x · goal popup',
      interpretation: null,
      at: 2,
      by: 'pi-x',
    },
  });
  // Follower: same canonical request, no delivery and no native alert.
  const follower = controller(
    attentionStore({coordinator: 'pi-other-01', requests: [answered]}),
  );
  await follower.ctl.setScope([goalRef]);
  assert.deepEqual(follower.delivered, []);
  // Held coordinator: paused stays paused.
  const held = controller(
    attentionStore({
      coordinator: agent.owner,
      status: 'held',
      requests: [answered],
    }),
  );
  await held.ctl.setScope([goalRef]);
  assert.deepEqual(held.delivered, []);
  // Coordinator: delivered once; the notice is the branch receipt.
  const store = attentionStore({
    coordinator: agent.owner,
    requests: [answered],
  });
  const owner = controller(store);
  await owner.ctl.setScope([goalRef]);
  await owner.ctl.refresh();
  assert.equal(owner.delivered.length, 1);
  assert.equal(owner.delivered[0].customType, ANSWER_MESSAGE);
  assert.deepEqual(owner.delivered[0].details, {requests: [requestId(1)]});
  assert.match(owner.delivered[0].content, /incorporate/);
  assert.deepEqual([...deliveredAnswers(owner.branch)], [requestId(1)]);
  // Reload on the same branch: the receipt prevents a repeat.
  const reloaded = controller(store, {branch: owner.branch});
  await reloaded.ctl.setScope([goalRef]);
  assert.equal(reloaded.delivered.length, 0);
  // Crash after persistence, before delivery: a reload without a receipt delivers.
  const recovered = controller(store, {branch: []});
  await recovered.ctl.setScope([goalRef]);
  assert.equal(recovered.delivered.length, 1);
  // A fresh runtime is not the coordinator; the saved answer waits.
  const fresh = controller(store, {
    branch: [],
    agent: {session: 's2', owner: 'pi-s2-ff'},
  });
  await fresh.ctl.setScope([goalRef]);
  assert.equal(fresh.delivered.length, 0);
});

test('native attention is signalled by the coordinator only, and only when it changes', async () => {
  const store = attentionStore({
    coordinator: agent.owner,
    requests: [request()],
  });
  const owner = controller(store);
  await owner.ctl.setScope([goalRef]);
  await owner.ctl.refresh();
  await owner.ctl.refresh();
  assert.deepEqual(owner.signals, [
    {
      requests: [
        {
          id: requestId(1),
          title: 'Choose the rollout scope',
        },
      ],
    },
  ]);
  await owner.ctl.answer(requestId(1), {choice: 'one'});
  assert.deepEqual(
    owner.signals.at(-1),
    {requests: []},
    'answered is no longer a person’s',
  );
  const follower = controller(
    attentionStore({coordinator: 'pi-other', requests: [request()]}),
  );
  await follower.ctl.setScope([goalRef]);
  assert.deepEqual(follower.signals, []);
  assert.equal(follower.ctl.model().items.length, 1, 'followers still see it');
});

test('an empty view reads only responsibilities and a failed read is unavailable, not empty', async () => {
  const store = attentionStore({requests: [request()]});
  const {ctl} = controller(store);
  await ctl.setScope([]);
  assert.deepEqual(
    store.state.calls.map(call => [call.op, call.owner]),
    [['coordinating', agent.owner]],
    'nothing is listed without follows or coordination',
  );
  assert.deepEqual(ctl.coordinated(), {goals: [], failure: undefined});
  assert.equal(
    store.state.calls[0].actor.env.DOTS_AGENT_OWNER,
    agent.owner,
    'the read acts as this runtime',
  );
  assert.deepEqual(ctl.model().items, []);
  await ctl.setScope([goalRef]);
  store.state.fail = 'catalog unreadable';
  await ctl.refresh();
  assert.match(ctl.model().unavailable.join(), /catalog unreadable/);
});

const external = (store, id, choice = 'one') =>
  store.source.answer(
    PROJECT,
    id,
    {source: 'ui', choice, provenance: 'dots attention view'},
    {env: {}},
  );
const notices = delivered =>
  delivered.filter(message => message.customType === ANSWER_MESSAGE);

test('unfollowing a coordinated goal keeps it actionable, delivered and in context until released', async () => {
  const store = attentionStore({
    coordinator: agent.owner,
    requests: [request()],
    mandate: decision({kind: 'mandate'}),
  });
  const {ctl, delivered, signals} = controller(store);
  await ctl.setScope([goalRef]);
  await ctl.setScope([]);
  assert.deepEqual(ctl.goals(), [goalRef], 'responsibility keeps it in scope');
  assert.deepEqual(
    ctl.model().items.map(item => item.request.id),
    [requestId(1)],
    'still answerable here',
  );
  assert.equal(
    indicator(ctl.model()),
    'Needs your attention: Choose the rollout scope',
  );
  assert.deepEqual(signals.at(-1), {
    requests: [{id: requestId(1), title: 'Choose the rollout scope'}],
  });
  await external(store, requestId(1));
  await ctl.refresh();
  await ctl.refresh();
  const sent = notices(delivered);
  assert.equal(sent.length, 1, 'delivered once');
  assert.deepEqual(sent[0].details, {requests: [requestId(1)]});
  assert.doesNotMatch(sent[0].content, /goal_focus|not follow/);
  assert.deepEqual(signals.at(-1), {requests: []});
  const facts = await ctl.facts();
  assert.deepEqual([...facts.keys()], [`${PROJECT}:t-123abc`]);
  assert.deepEqual(ctl.coordinated(), {goals: [goalRef], failure: undefined});
  const context = attentionContext(
    goalRef,
    facts.get(`${PROJECT}:t-123abc`),
    agent.owner,
  );
  assert.doesNotMatch(context, /not followed/);
  assert.match(context, /Current mandate d-0+1:\nShip behind a flag\./);
  assert.match(
    context,
    /Human input \(chat, pi session s · interactive input\): ship it behind a flag/,
  );
  assert.match(context, /Saved answer, not yet incorporated — r-0+1/);
  // Release ends responsibility: the goal leaves scope and context.
  store.state.coordinator = null;
  await ctl.refresh();
  assert.deepEqual(ctl.goals(), []);
  assert.deepEqual(ctl.model().items, []);
  assert.equal((await ctl.facts()).size, 0);
});

test('a never-followed coordinated goal is in scope and delivers; release, takeover and pause remove it', async () => {
  const store = attentionStore({
    coordinator: agent.owner,
    requests: [request()],
  });
  const {ctl, delivered, signals} = controller(store);
  await ctl.refresh();
  assert.equal(signals.at(-1).requests[0].id, requestId(1));
  assert.equal(ctl.model().items.length, 1, 'answerable without a follow');
  await external(store, requestId(1));
  await ctl.refresh();
  assert.equal(notices(delivered).length, 1);
  for (const [change, label] of [
    [() => (store.state.coordinator = null), 'released'],
    [() => (store.state.coordinator = 'pi-other-runtime'), 'taken over'],
    [
      () => {
        store.state.coordinator = agent.owner;
        store.state.status = 'held';
      },
      'paused',
    ],
  ]) {
    const id = requestId(notices(delivered).length + 10);
    store.state.requests.set(id, request({id, key: id}));
    change();
    await ctl.refresh();
    assert.deepEqual(signals.at(-1), {requests: []}, `${label}: no alert`);
    assert.deepEqual(ctl.model().items, [], `${label}: out of scope`);
    await external(store, id);
    await ctl.refresh();
    assert.equal(notices(delivered).length, 1, `${label}: nothing delivered`);
  }
});

test('a failed coordination read is unknown: no delivery, the alert stays', async () => {
  const store = attentionStore({
    coordinator: agent.owner,
    requests: [request()],
  });
  const {ctl, delivered, signals} = controller(store);
  await ctl.refresh();
  const alert = signals.at(-1);
  assert.equal(alert.requests.length, 1);
  store.state.coordinatingFail = 'catalog locked';
  await external(store, requestId(1));
  await ctl.refresh();
  assert.equal(notices(delivered).length, 0, 'stale ownership is not acted on');
  assert.equal(signals.at(-1), alert, 'unknown is not nothing');
  assert.match(
    ctl.model().unavailable.join(),
    /Goals this session coordinates could not be checked: catalog locked/,
  );
  assert.deepEqual(
    ctl.coordinated(),
    {goals: [goalRef], failure: 'catalog locked'},
    'membership stays as last known',
  );
  assert.equal(ctl.model().items.length, 1, 'still shown, not acted on');
  store.state.coordinatingFail = undefined;
  await ctl.refresh();
  assert.equal(notices(delivered).length, 1);
  assert.deepEqual(signals.at(-1), {requests: []});
});

test('a coordinated goal the store cannot read neither delivers nor clears its alert', async () => {
  const store = attentionStore({
    coordinator: agent.owner,
    requests: [request()],
  });
  const {ctl, delivered, signals} = controller(store);
  await ctl.refresh();
  const alert = signals.at(-1);
  const list = store.source.list;
  store.source.list = async (goals, signal) => ({
    ...(await list([], signal)),
    goals: [],
    errors: goals.map(goal => ({
      project: goal.project,
      goal: goal.id,
      error: 'project is not registered',
    })),
  });
  await external(store, requestId(1));
  await ctl.refresh();
  assert.equal(notices(delivered).length, 0);
  assert.equal(signals.at(-1), alert);
  assert.deepEqual(ctl.model().unavailable, [
    't-123abc: project is not registered',
  ]);
  assert.deepEqual(ctl.model().items, []);
});

test('managed workers never read coordination', async () => {
  const store = attentionStore({coordinator: agent.owner});
  const ctl = new AttentionController({
    source: store.source,
    deliver() {},
    branch: () => [],
    changed() {},
    signal() {},
    coordinates: false,
  });
  ctl.bind(agent);
  await ctl.refresh();
  assert.deepEqual(store.state.calls, []);
});

test('late reads from an earlier scope never land', async () => {
  const store = attentionStore({requests: [request()]});
  let release;
  const list = store.source.list;
  store.source.list = async (goals, signal) => {
    if (goals.length === 1 && goals[0].id === 't-123abc')
      await new Promise(resolve => (release = resolve));
    return list(goals, signal);
  };
  const {ctl} = controller(store);
  const first = ctl.setScope([goalRef]);
  await settle();
  await ctl.setScope([]);
  release();
  await first;
  assert.deepEqual(ctl.model().items, []);
});

test('follows the shared stream from head and refreshes on attention events', async () => {
  const store = attentionStore({requests: []});
  const follows = [];
  const ctl = new AttentionController({
    source: store.source,
    deliver() {},
    branch: () => [],
    changed() {},
    signal() {},
    tasks: {
      head: async () => 41,
      follow(project, root, since, listener) {
        const entry = {project, root, since, listener, stopped: false};
        follows.push(entry);
        return () => (entry.stopped = true);
      },
    },
  });
  ctl.bind(agent);
  await ctl.setScope([goalRef]);
  await settle();
  assert.equal(follows.length, 1);
  assert.equal(follows[0].since(), 41);
  store.state.requests.set(requestId(1), request());
  follows[0].listener.event({
    seq: 42,
    task: 't-456def',
    at: 1,
    actor: null,
    kind: 'request-raised',
    detail: null,
    result: null,
  });
  await settle();
  await settle();
  assert.equal(ctl.model().items.length, 1);
  assert.equal(follows[0].since(), 42);
  await ctl.setScope([]);
  assert.equal(follows[0].stopped, true);
});

test('the stream cursor is read before the snapshot, so an answer between them is delivered', async () => {
  const store = attentionStore({
    coordinator: agent.owner,
    requests: [request()],
  });
  let resolveHead;
  let heads = 0;
  const follows = [];
  const delivered = [];
  const ctl = new AttentionController({
    source: store.source,
    deliver: compose => delivered.push(compose()),
    branch: () => [],
    changed() {},
    signal() {},
    tasks: {
      head: () => {
        heads++;
        return new Promise(resolve => (resolveHead = resolve));
      },
      follow(project, root, since, listener) {
        follows.push({since, listener});
        return () => {};
      },
    },
  });
  ctl.bind(agent);
  try {
    const first = ctl.setScope([goalRef]);
    await settle();
    // An overlapping refresh waits on the same cursor read.
    const second = ctl.refresh();
    await settle();
    assert.equal(heads, 1);
    assert.equal(
      store.state.calls.some(call => call.op === 'list'),
      false,
      'no snapshot before the cursor',
    );
    await external(store, requestId(1));
    resolveHead(42);
    await Promise.all([first, second]);
    assert.equal(follows[0].since(), 42);
    assert.equal(notices(delivered).length, 1);
    assert.equal(ctl.model().items[0].state.kind, 'answered');
  } finally {
    ctl.close();
  }
});

test('a held answer notice is declined once a pause is observed, without a receipt, and resume delivers it', async () => {
  const store = attentionStore({
    coordinator: agent.owner,
    requests: [request()],
  });
  await external(store, requestId(1));
  let idle = false;
  const sent = [];
  const branch = [];
  const delivery = new TurnMessages(
    {
      on() {},
      sendMessage(message, options) {
        sent.push({message, options});
        branch.push({type: 'custom_message', ...message});
      },
    },
    5,
  );
  delivery.bind({isIdle: () => idle});
  const ctl = new AttentionController({
    source: store.source,
    deliver: (compose, done) => delivery.send(compose, done),
    branch: () => branch,
    changed() {},
    signal() {},
  });
  ctl.bind(agent);
  try {
    await ctl.setScope([goalRef]);
    assert.equal(delivery.holding(), true);
    store.state.status = 'held';
    await ctl.refresh();
    idle = true;
    await sleep(25);
    assert.deepEqual(sent, [], 'a paused goal starts nothing');
    assert.deepEqual([...deliveredAnswers(branch)], [], 'no receipt');
    // Explicit resume: the saved answer is delivered to the coordinator.
    store.state.status = 'open';
    await ctl.refresh();
    await sleep(25);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].options.triggerTurn, true);
    assert.deepEqual(sent[0].message.details, {requests: [requestId(1)]});
    // A handoff while held declines the notice the same way.
    const id = requestId(2);
    store.state.requests.set(id, request({id, key: id}));
    await external(store, id);
    idle = false;
    await ctl.refresh();
    store.state.coordinator = 'pi-other-runtime';
    await ctl.refresh();
    idle = true;
    await sleep(25);
    assert.equal(sent.length, 1);
  } finally {
    ctl.close();
    delivery.close();
  }
});

test('a drafted request that leaves the list stays with its draft beside its replacement until dismissed', async () => {
  const store = attentionStore({
    requests: [
      request(),
      request({id: requestId(2), key: 'other', created_at: 200}),
    ],
  });
  const {ctl} = controller(store);
  await ctl.setScope([goalRef]);
  ctl.drafts.set(requestId(1), 'my private draft');
  ctl.drafts.set(requestId(2), '  ');
  store.supersede(
    requestId(1),
    request({id: requestId(3), question: 'A changed question'}),
  );
  store.supersede(requestId(2), request({id: requestId(4), key: 'other'}));
  await ctl.refresh();
  await ctl.refresh();
  const byId = new Map(ctl.model().items.map(item => [item.request.id, item]));
  assert.deepEqual(
    [...byId.keys()].sort(),
    [requestId(1), requestId(3), requestId(4)],
    'only a non-empty draft keeps its original',
  );
  const original = byId.get(requestId(1));
  assert.equal(original.state.kind, 'stale');
  assert.equal(original.state.replacement.id, requestId(3));
  assert.equal(
    original.request.question,
    'Should the first slice cover every project or only this one?',
  );
  assert.equal(ctl.drafts.get(requestId(1)), 'my private draft');
  assert.equal(byId.get(requestId(3)).state.kind, 'open', 'never retargeted');
  assert.equal(ctl.drafts.has(requestId(3)), false);
  // Withdrawn without a replacement: kept as no longer outstanding.
  ctl.drafts.set(requestId(3), 'second draft');
  store.state.requests.get(requestId(3)).state = 'closed';
  await ctl.refresh();
  assert.equal(
    ctl.model().items.find(item => item.request.id === requestId(3)).state.kind,
    'closed',
  );
  ctl.dismiss(requestId(1));
  assert.equal(ctl.drafts.has(requestId(1)), false);
  assert.equal(
    ctl.model().items.some(item => item.request.id === requestId(1)),
    false,
  );
  // Leaving scope takes local outcomes with it.
  await ctl.setScope([]);
  assert.deepEqual(ctl.model().items, []);
});

test('a drafted request survives an unreadable goal and is decided when the goal reads again', async () => {
  const store = attentionStore({
    requests: [
      request(),
      request({id: requestId(2), key: 'other', created_at: 200}),
    ],
  });
  const {ctl} = controller(store);
  await ctl.setScope([goalRef]);
  ctl.drafts.set(requestId(1), 'my private draft');
  ctl.drafts.set(requestId(2), 'still open draft');
  const list = store.source.list;
  const unreadable = async (goals, signal) => ({
    ...(await list([], signal)),
    errors: goals.map(goal => ({
      project: goal.project,
      goal: goal.id,
      error: 'database is locked',
    })),
  });
  store.source.list = unreadable;
  await ctl.refresh();
  await ctl.refresh();
  assert.deepEqual(ctl.model().items, [], 'unread is not closed');
  assert.deepEqual(ctl.model().unavailable, ['t-123abc: database is locked']);
  store.supersede(
    requestId(1),
    request({id: requestId(3), question: 'A changed question'}),
  );
  store.source.list = list;
  await ctl.refresh();
  const byId = new Map(ctl.model().items.map(item => [item.request.id, item]));
  const original = byId.get(requestId(1));
  assert.equal(original.state.kind, 'stale');
  assert.equal(original.state.replacement.id, requestId(3));
  assert.equal(
    original.request.question,
    'Should the first slice cover every project or only this one?',
  );
  assert.equal(ctl.drafts.get(requestId(1)), 'my private draft');
  assert.equal(byId.get(requestId(3)).state.kind, 'open', 'never retargeted');
  assert.equal(
    byId.get(requestId(2)).state.kind,
    'open',
    'still listed: the store decides',
  );
  // The held snapshot is gone once read, so a later supersede still applies.
  store.supersede(requestId(2), request({id: requestId(4), key: 'other'}));
  await ctl.refresh();
  assert.equal(
    ctl.model().items.find(item => item.request.id === requestId(2)).state.kind,
    'stale',
  );
  // Leaving scope during an outage still takes local outcomes with it.
  ctl.drafts.set(requestId(3), 'third draft');
  store.source.list = unreadable;
  await ctl.refresh();
  await ctl.setScope([]);
  store.source.list = list;
  store.state.requests.get(requestId(3)).state = 'closed';
  await ctl.setScope([goalRef]);
  assert.equal(
    ctl.model().items.some(item => item.request.id === requestId(3)),
    false,
  );
});

test('context carries the mandate with its human basis kept apart, bounded decisions and roles', () => {
  const linked = decision({
    id: decisionId(9),
    kind: 'mandate',
    decision: 'Migrate this project first; others follow after review.',
    human: null,
    request: requestId(1),
  });
  const source = request({
    state: 'closed',
    answer: {
      choice: 'one',
      text: null,
      source: 'ui',
      provenance: 'pi session s · goal popup',
      interpretation: null,
      at: 5,
      by: null,
    },
  });
  const decisions = Array.from({length: 7}, (_, n) =>
    decision({id: decisionId(n + 1), decision: `Decision ${n + 1}`, at: n}),
  );
  const facts = {
    goal: listed([request({id: requestId(4), title: 'Approve the release'})], {
      mandate: linked,
      coordinator: agent.owner,
    }).goals[0],
    decisions: [...decisions, linked],
    mandateRequest: source,
    failure: undefined,
  };
  const text = attentionContext(goalRef, facts, agent.owner);
  assert.match(text, /This session coordinates the goal/);
  assert.match(text, /Current mandate d-0+9:\nMigrate this project first/);
  assert.match(text, /From request r-0+1\. Question: Should the first slice/);
  assert.match(
    text,
    /Answer \(ui, pi session s · goal popup\): This project only/,
  );
  assert.match(text, /Decision 7[\s\S]*Decision 3/);
  assert.doesNotMatch(text, /Decision 2\b/);
  assert.match(text, /2 earlier decisions not shown/);
  assert.match(
    text,
    /dots attention decisions --scope goals --goal p-a+:t-123abc/,
  );
  assert.match(text, /Awaiting a person — r-0+4/);
  assert.equal(
    attentionContext(goalRef, facts, agent.owner),
    text,
    'deterministic',
  );
  const follower = attentionContext(goalRef, facts, 'pi-someone-else');
  assert.match(follower, /not this session: do not incorporate/);
  const held = attentionContext(
    goalRef,
    {...facts, goal: {...facts.goal, status: 'held'}},
    agent.owner,
  );
  assert.match(held, /held \(paused\): do not dispatch or resume/);
  const bare = attentionContext(
    goalRef,
    {
      ...facts,
      goal: {...facts.goal, mandate: decision({kind: 'mandate', human: null})},
      mandateRequest: undefined,
    },
    agent.owner,
  );
  assert.match(
    bare,
    /No recorded human input; do not treat this wording as consent/,
  );
  assert.match(
    attentionContext(
      goalRef,
      {
        goal: undefined,
        decisions: undefined,
        mandateRequest: undefined,
        failure: 'offline',
      },
      agent.owner,
    ),
    /Unavailable: offline/,
  );
});

test('chat records need the person’s verbatim interactive words', async () => {
  const projectOf = id => (id === requestId(1) ? PROJECT : undefined);
  const humans = new HumanInputs();
  humans.observe({source: 'rpc', text: 'go with every project'});
  humans.observe({source: 'extension', text: 'go with every project'});
  humans.observe({
    source: 'interactive',
    text: 'Hmm. Only this project for now, please.',
  });
  const store = attentionStore({requests: [request()]});
  const signal = new AbortController().signal;
  await assert.rejects(
    recordChat(
      {
        kind: 'answer',
        quote: 'go with every project',
        request: requestId(1),
        interpretation: 'all',
      },
      agent,
      humans,
      store.source,
      [goalRef],
      projectOf,
      signal,
    ),
    /does not appear verbatim/,
  );
  await assert.rejects(
    recordChat(
      {
        kind: 'answer',
        quote: 'Only this project for now',
        request: requestId(1),
      },
      agent,
      humans,
      store.source,
      [goalRef],
      projectOf,
      signal,
    ),
    /needs request and interpretation/,
  );
  const text = await recordChat(
    {
      kind: 'answer',
      quote: 'Only this project for now',
      request: requestId(1),
      choice: 'one',
      interpretation: 'Chose this project only.',
    },
    agent,
    humans,
    store.source,
    [goalRef],
    projectOf,
    signal,
  );
  assert.match(text, /Saved the answer/);
  assert.deepEqual(store.state.calls.at(-1).content, {
    source: 'chat',
    text: 'Only this project for now',
    choice: 'one',
    provenance: 'pi session fixture-session · interactive input',
    interpretation: 'Chose this project only.',
  });
  await assert.rejects(
    recordChat(
      {
        kind: 'mandate',
        quote: 'Only this project',
        goal: 't-999',
        decision: 'x',
      },
      agent,
      humans,
      store.source,
      [goalRef],
      projectOf,
      signal,
    ),
    /not followed/,
  );
  await recordChat(
    {
      kind: 'mandate',
      quote: 'Only this project',
      goal: 't-123abc',
      decision: 'Scope the goal to this project.',
    },
    agent,
    humans,
    store.source,
    [goalRef],
    projectOf,
    signal,
  );
  assert.deepEqual(store.state.calls.at(-1).content.human, {
    text: 'Only this project',
    source: 'chat',
    provenance: 'pi session fixture-session · interactive input',
  });
  humans.clear();
  assert.equal(humans.attests('Only this project'), false);
});

test('status shows coordinator attention as a question, after a running agent', () => {
  const base = {
    running: false,
    compacting: false,
    rest: {state: 'idle'},
    activities: new Map(),
  };
  const attention = [
    {id: requestId(1), title: 'Choose the rollout scope'},
    {id: requestId(2), title: 'Other'},
  ];
  assert.deepEqual(rootRecord({...base, attention}), {
    state: 'blocked',
    kind: 'question',
    message: 'Needs your attention: Choose the rollout scope (+1 more)',
  });
  assert.equal(
    rootRecord({...base, running: true, attention}).state,
    'working',
  );
  assert.equal(rootRecord({...base, attention: []}).state, 'idle');
});
