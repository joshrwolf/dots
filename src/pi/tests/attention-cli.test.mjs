// The attention slice against the real candidate `dots` CLI in a disposable
// home: raise and supersede through the CLI, then read, answer, deliver,
// record and build context through the Pi controller and source.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdtempSync, mkdirSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {dotsSource} from '../extensions/goal/source.ts';
import {dotsAttention} from '../extensions/goal/attention/source.ts';
import {AttentionController} from '../extensions/goal/attention/controller.ts';
import {attentionContext} from '../extensions/goal/attention/context.ts';
import {HumanInputs, recordChat} from '../extensions/goal/attention/chat.ts';
import {agentEnv} from '../src/host/agent.ts';

const ISOLATED = [
  'HOME',
  'XDG_STATE_HOME',
  'XDG_DATA_HOME',
  'PI_CODING_AGENT_DIR',
];
// The fixture acts as its own runtime, never as an agent that ran the tests.
const INHERITED = ['DOTS_WORKER_ID', 'DOTS_AGENT_OWNER', 'PI_SESSION_ID'];

const agent = {session: 'fixture', owner: 'pi-fixture-0123456789abcdef'};

/** A disposable home with one registered project, for one test's body. */
async function withHome(body) {
  const dir = mkdtempSync(join(tmpdir(), 'dots-attention-cli-'));
  const saved = [...ISOLATED, ...INHERITED].map(key => [key, process.env[key]]);
  for (const key of ISOLATED) process.env[key] = join(dir, key);
  for (const key of INHERITED) delete process.env[key];
  let project;
  const run = (args, owner = agent.owner) =>
    JSON.parse(
      execFileSync(
        'dots',
        [
          '--json',
          '--directory',
          dir,
          ...args,
          '--project',
          project,
          '--owner',
          owner,
        ],
        {encoding: 'utf8'},
      ),
    );
  const content = (name, value) => {
    const file = join(dir, name);
    writeFileSync(file, JSON.stringify(value));
    return file;
  };
  try {
    mkdirSync(process.env.HOME, {recursive: true});
    execFileSync('git', ['init', '--quiet', dir]);
    project = JSON.parse(
      execFileSync(
        'dots',
        ['--json', '--directory', dir, 'project', 'init', 'Fixture', '--fresh'],
        {encoding: 'utf8'},
      ),
    ).project.id;
    const brief = join(
      process.env.XDG_DATA_HOME,
      'dots/workspaces',
      project,
      'briefs/brief.md',
    );
    mkdirSync(join(brief, '..'), {recursive: true});
    writeFileSync(brief, '# Fixture goal');
    await body({dir, project, run, content});
  } finally {
    for (const [key, value] of saved) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
    rmSync(dir, {recursive: true, force: true});
  }
}

test('a request moves from raise to answer, delivery, context and incorporation', async () => {
  await withHome(async ({dir, project, run, content}) => {
    const id = run([
      'task',
      'goal',
      'create',
      'Fixture',
      '--brief',
      'briefs/brief.md',
    ]).goal.root.id;
    run(['task', 'goal', 'coordinate', id]);
    const slice = run(['task', 'add', 'Migrate', '--parent', id]).task.id;
    const raised = run([
      'attention',
      'raise',
      id,
      '--key',
      'scope',
      '--content-file',
      content('raise.json', {
        kind: 'decision',
        title: 'Choose the rollout scope',
        question: 'This project only, or every project?',
        choices: [
          {id: 'one', label: 'This project only'},
          {id: 'all', label: 'Every project'},
        ],
        tasks: [slice],
      }),
    ]);
    const first = raised.request.id;
    const goal = {project, id};

    // Holds come from dots, not a client-side engine.
    const shown = await dotsSource.show(
      dir,
      id,
      new AbortController().signal,
      false,
      project,
    );
    const held = shown.overview.tasks.find(task => task.id === slice);
    assert.deepEqual(held.holds, [
      {kind: 'request', request: first, task: slice, state: 'open'},
    ]);

    const source = dotsAttention();
    const branch = [];
    const signals = [];
    const controller = new AttentionController({
      source,
      deliver: compose => branch.push({type: 'custom_message', ...compose()}),
      branch: () => branch,
      changed() {},
      signal: signal => signals.push(signal),
    });
    controller.bind(agent);
    await controller.setScope([goal]);
    assert.deepEqual(
      controller
        .model()
        .items.map(item => [item.request.id, item.coordination]),
      [[first, 'self']],
    );
    assert.deepEqual(signals.at(-1), {
      requests: [
        {
          id: first,
          title: 'Choose the rollout scope',
          goal: 'Fixture',
          followed: true,
        },
      ],
    });

    // A replaced request keeps the draft and never answers the replacement.
    const second = run([
      'attention',
      'raise',
      id,
      '--key',
      'naming',
      '--content-file',
      content('naming.json', {
        kind: 'decision',
        title: 'Name the package',
        question: 'Atlas or Beacon?',
      }),
    ]).request.id;
    await controller.refresh();
    run([
      'attention',
      'supersede',
      second,
      '--content-file',
      content('renamed.json', {
        kind: 'decision',
        title: 'Name the package',
        question: 'Atlas, Beacon or Compass?',
      }),
    ]);
    controller.drafts.set(second, 'Atlas');
    await controller.answer(second, {text: 'Atlas'});
    const stale = controller
      .model()
      .items.find(item => item.request.id === second);
    assert.equal(stale.state.kind, 'stale');
    assert.equal(stale.state.replacement.question, 'Atlas, Beacon or Compass?');
    assert.equal(controller.drafts.get(second), 'Atlas');
    assert.equal(stale.state.replacement.answer, null);

    // A UI answer persists through the CLI and is delivered once.
    await controller.answer(first, {choice: 'one'});
    const stored = await source.show(
      project,
      first,
      new AbortController().signal,
    );
    assert.equal(stored.state, 'answered');
    assert.equal(stored.answer.source, 'ui');
    assert.equal(stored.answer.provenance, 'pi session fixture · goal popup');
    assert.equal(stored.answer.by, agent.owner);
    const notices = branch.filter(
      entry => entry.customType === 'dots.attention.answer',
    );
    assert.equal(notices.length, 1);
    assert.deepEqual(notices[0].details, {requests: [first]});
    await controller.refresh();
    assert.equal(branch.length, 1, 'the notice is the receipt');

    // A chat mandate rests on the person's own words.
    const humans = new HumanInputs();
    humans.observe({
      source: 'interactive',
      text: 'Keep it to this project until review.',
    });
    await recordChat(
      {
        kind: 'mandate',
        quote: 'Keep it to this project until review.',
        goal: id,
        decision: 'Scope the goal to this project until review.',
      },
      agent,
      humans,
      source,
      [goal],
      request => controller.projectOf(request),
      new AbortController().signal,
    );
    const facts = (await controller.facts()).get(`${project}:${id}`);
    const context = attentionContext(goal, facts, agent.owner);
    assert.match(context, /This session coordinates the goal/);
    assert.match(
      context,
      /Current mandate d-[0-9a-f]+:\nScope the goal to this project until review\./,
    );
    assert.match(
      context,
      /Human input \(chat, pi session fixture · interactive input\): Keep it to this project until review\./,
    );
    assert.match(
      context,
      new RegExp(`Saved answer, not yet incorporated — ${first}`),
    );

    // Incorporation closes the request and lifts the hold.
    run([
      'attention',
      'incorporate',
      first,
      '--content-file',
      content('incorporate.json', {
        kind: 'decision',
        decision: 'Migrate this project first.',
      }),
    ]);
    await controller.refresh();
    assert.equal(
      controller.model().items.some(item => item.request.id === first),
      false,
    );
    const lifted = await dotsSource.show(
      dir,
      id,
      new AbortController().signal,
      false,
      project,
    );
    assert.deepEqual(
      lifted.overview.tasks.find(task => task.id === slice).holds,
      [],
    );
    const decisions = await source.decisions(
      [goal],
      new AbortController().signal,
    );
    assert.deepEqual(
      decisions.goals[0].decisions.map(item => [
        item.kind,
        item.request === first,
      ]),
      [
        ['mandate', false],
        ['decision', true],
      ],
    );
    controller.close();
  });
});

test('root claims in the store decide delivery, followed or not', async () => {
  await withHome(async ({project, run, content}) => {
    const source = dotsAttention();
    const signal = new AbortController().signal;
    const create = title =>
      run(['task', 'goal', 'create', title, '--brief', 'briefs/brief.md']).goal
        .root.id;
    const coordinated = create('Coordinated');
    const other = create('Elsewhere');
    run(['task', 'goal', 'coordinate', coordinated]);
    run(['task', 'goal', 'coordinate', other], 'pi-other-fedcba9876543210');
    assert.deepEqual(
      await source.coordinating(agent.owner, {env: agentEnv(agent)}, signal),
      [{project, id: coordinated}],
      'exactly the root this runtime claims',
    );
    const raise = (goal, key) =>
      run(
        [
          'attention',
          'raise',
          goal,
          '--key',
          key,
          '--content-file',
          content(`${key}.json`, {
            kind: 'decision',
            title: `Decide ${key}`,
            question: `Which ${key}?`,
            choices: [{id: 'a', label: 'A'}],
          }),
        ],
        goal === other ? 'pi-other-fedcba9876543210' : agent.owner,
      ).request.id;
    const answer = id =>
      run(
        [
          'attention',
          'answer',
          id,
          '--content-file',
          content('answer.json', {
            source: 'ui',
            choice: 'a',
            provenance: 'dots attention view · fixture terminal',
          }),
        ],
        'viewer-fixture',
      );
    const first = raise(coordinated, 'first');
    raise(other, 'theirs');
    const branch = [];
    const signals = [];
    const controller = new AttentionController({
      source,
      deliver: compose => branch.push({type: 'custom_message', ...compose()}),
      branch: () => branch,
      changed() {},
      signal: next => signals.push(next),
    });
    controller.bind(agent);
    // Never followed: the view is empty, the coordinator still alerts.
    await controller.refresh();
    assert.deepEqual(controller.model().items, []);
    assert.deepEqual(signals.at(-1), {
      requests: [
        {
          id: first,
          title: 'Decide first',
          goal: 'Coordinated',
          followed: false,
        },
      ],
    });
    answer(first);
    await controller.refresh();
    await controller.refresh();
    assert.deepEqual(
      branch.map(entry => entry.details),
      [{requests: [first]}],
      'delivered once, without following',
    );
    // Taken over: responsibility leaves with the root claim.
    const second = raise(coordinated, 'second');
    run(
      ['task', 'goal', 'coordinate', coordinated, '--take'],
      'pi-other-fedcba9876543210',
    );
    answer(second);
    await controller.refresh();
    assert.equal(branch.length, 1);
    assert.deepEqual(signals.at(-1), {requests: []});
    assert.deepEqual(
      await source.coordinating(agent.owner, {env: agentEnv(agent)}, signal),
      [],
    );
    controller.close();
  });
});
