import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdtempSync, realpathSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import register, {NOTICE} from '../extensions/review/index.ts';

const settle = (ms = 40) => new Promise(resolve => setTimeout(resolve, ms));

/** Hand-authored review state answering `dots review list` and `show` as the CLI does. */
function fakeDots() {
  const reviews = new Map();
  const calls = [];
  const add = (id, owner, worktree = '/repo/linked') => {
    const review = {
      id,
      owner,
      worktree: {path: worktree, branch: 'slice'},
      threads: [],
    };
    reviews.set(id, review);
    return review;
  };
  const thread = (review, id, anchor, ...comments) => {
    const entry = {
      id,
      anchor,
      status: 'open',
      comments: comments.map(([author, body, status = null]) => ({
        author,
        at: 1,
        body,
        status,
      })),
    };
    review.threads.push(entry);
    return entry;
  };
  const comment = (entry, author, body, status = null) =>
    entry.comments.push({author, at: 2, body, status});
  const run = async args => {
    calls.push(args);
    if (args[1] === 'list') {
      const owner = args[args.indexOf('--owner') + 1];
      return {
        reviews: [...reviews.values()]
          .filter(review => review.owner === owner)
          .map(review => ({
            id: review.id,
            owner: review.owner,
            file: `${review.worktree.path}/.git/dots/reviews/${review.id}.json`,
          })),
      };
    }
    if (args[1] === 'show') {
      const review = reviews.get(args[2]);
      if (!review) throw new Error(`no review ${args[2]} in this repository`);
      return {review: structuredClone(review)};
    }
    throw new Error(`unexpected dots ${args.join(' ')}`);
  };
  return {reviews, calls, add, thread, comment, run};
}

/** A Pi runtime: lifecycle handlers, sent messages, and the branch they land on. */
function runtime(
  dotsRun,
  {session = 's1', branch = [], realWatch = false} = {},
) {
  const listeners = {};
  const handlers = {};
  const messages = [];
  const watches = new Map();
  const notices = [];
  const state = {idle: true};
  const pi = {
    on: (name, fn) => {
      (listeners[name] ??= []).push(fn);
      handlers[name] = (event, context) => {
        let result;
        for (const listener of listeners[name])
          result = listener(event, context);
        return result;
      };
    },
    // Sent while idle, Pi appends the message or starts a run with it.
    sendMessage: (message, options) => {
      messages.push({message, options});
      branch.push({type: 'custom_message', ...message});
    },
  };
  register(pi, {
    dots: dotsRun,
    settleMs: 5,
    watch: realWatch
      ? undefined
      : (directory, changed) => {
          watches.set(directory, changed);
          return {close: () => watches.delete(directory)};
        },
  });
  const ctx = {
    cwd: '/repo/checkout',
    hasUI: true,
    ui: {notify: (message, level) => notices.push({message, level})},
    isIdle: () => state.idle,
    sessionManager: {
      getSessionId: () => session,
      getBranch: () => branch,
    },
  };
  return {
    handlers,
    messages,
    watches,
    notices,
    state,
    branch,
    ctx,
    start: () => handlers.session_start({type: 'session_start'}, ctx),
    shutdown: () => handlers.session_shutdown({type: 'session_shutdown'}, ctx),
    /** Every watched directory reports a change. */
    changed: () => {
      for (const changed of watches.values()) changed();
    },
    /** The current run settles. */
    settled(aborted = false) {
      state.idle = true;
      handlers.agent_settled({type: 'agent_settled', aborted}, ctx);
    },
  };
}

test('a user reply is delivered once, starting a turn', async () => {
  const fake = fakeDots();
  const review = fake.add('r-00000001', 'pi-s1');
  const t1 = fake.thread(
    review,
    't1',
    {path: 'src/lib.rs', side: 'new', start: 10, end: 12},
    ['claude-opus', 'Handle the empty case'],
  );
  const h = runtime(fake.run);
  h.start();
  await settle();
  assert.deepEqual(fake.calls[0], [
    'review',
    'list',
    '--all',
    '--owner',
    'pi-s1',
  ]);
  assert.deepEqual([...h.watches.keys()], ['/repo/linked/.git/dots/reviews']);
  assert.equal(h.messages.length, 0, 'agent findings are not replies');

  fake.comment(t1, 'user', 'Why not\nreuse the helper?');
  h.changed();
  await settle();
  assert.equal(h.messages.length, 1);
  const [{message, options}] = h.messages;
  assert.deepEqual(options, {triggerTurn: true});
  assert.equal(message.customType, NOTICE);
  assert.equal(message.display, true);
  assert.deepEqual(message.details, {marks: {'r-00000001/t1': 2}});
  assert.equal(
    message.content,
    [
      'The user responded in a review this session started.',
      [
        'r-00000001 in /repo/linked (slice)',
        '- t1 · src/lib.rs:10-12 · user replied: Why not reuse the helper?',
        'Read: dots review show r-00000001 (t1)',
      ].join('\n'),
      'Decide whether to answer, fix, or resume the worker that owns the change; then reply on each thread with the outcome (dots review reply ID THREAD --author <you> --body ...).',
    ].join('\n\n'),
  );

  fake.comment(t1, 'claude-opus', 'Fixed in the next revision', 'fixed');
  h.changed();
  await settle();
  assert.equal(h.messages.length, 1, 'neither repeats nor reports agents');

  fake.comment(t1, 'user', null, 'dismissed');
  h.changed();
  await settle();
  assert.equal(h.messages.length, 2);
  assert.match(
    h.messages[1].message.content,
    /- t1 · src\/lib\.rs:10-12 · user set dismissed\n/,
  );
  assert.deepEqual(h.messages[1].message.details.marks, {
    'r-00000001/t1': 4,
  });
  await h.shutdown();
  assert.equal(h.watches.size, 0);
});

test('a burst across threads and reviews becomes one notice', async () => {
  const fake = fakeDots();
  const first = fake.add('r-00000001', 'pi-s1');
  const second = fake.add('r-00000002', 'pi-s1', '/repo/checkout');
  const h = runtime(fake.run);
  h.start();
  await settle();
  fake.thread(first, 't1', null, ['user', 'Is this the right approach?']);
  h.changed();
  fake.thread(first, 't2', {path: 'a.rs', side: 'old', start: 3, end: 3}, [
    'user',
    'Keep this',
  ]);
  h.changed();
  fake.thread(second, 't1', null, ['user', 'Also here']);
  h.changed();
  await settle();
  assert.equal(h.messages.length, 1);
  const {content, details} = h.messages[0].message;
  assert.match(
    content,
    /- t1 · review-level · user opened a thread: Is this the right approach\?\n- t2 · a\.rs:3 \(old side\) · user opened a thread: Keep this\nRead: dots review show r-00000001 \(t1, t2\)/,
  );
  assert.match(content, /r-00000002 in \/repo\/checkout \(slice\)/);
  assert.deepEqual(details.marks, {
    'r-00000001/t1': 1,
    'r-00000001/t2': 1,
    'r-00000002/t1': 1,
  });
  await h.shutdown();
});

test('reload and resume neither repeat a notice nor lose one that arrived while closed', async () => {
  const fake = fakeDots();
  const review = fake.add('r-00000001', 'pi-s1');
  const t1 = fake.thread(review, 't1', null, ['user', 'First']);
  const branch = [];
  const before = runtime(fake.run, {branch});
  before.start();
  await settle();
  assert.equal(before.messages.length, 1);
  await before.shutdown();

  const reloaded = runtime(fake.run, {branch});
  reloaded.start();
  await settle();
  assert.equal(reloaded.messages.length, 0, 'the branch holds the receipt');
  await reloaded.shutdown();

  fake.comment(t1, 'user', 'Second, while Pi was closed');
  const resumed = runtime(fake.run, {branch});
  resumed.start();
  await settle();
  assert.equal(resumed.messages.length, 1);
  assert.match(resumed.messages[0].message.content, /Second, while Pi/);
  assert.doesNotMatch(resumed.messages[0].message.content, /First/);
  await resumed.shutdown();
});

test('a notice waits for a busy session without repeating, and an empty branch is told again', async () => {
  const fake = fakeDots();
  const review = fake.add('r-00000001', 'pi-s1');
  fake.thread(review, 't1', null, ['user', 'Look here']);
  const h = runtime(fake.run);
  h.state.idle = false;
  h.start();
  await settle();
  h.changed();
  await settle();
  assert.equal(h.messages.length, 0, 'held while the run continues');
  h.settled();
  await settle();
  assert.equal(h.messages.length, 1, 'delivered once the run settles');
  assert.deepEqual(h.messages[0].options, {triggerTurn: true});

  h.branch.length = 0;
  h.handlers.session_tree({type: 'session_tree'}, h.ctx);
  await settle();
  assert.equal(h.messages.length, 2, 'the new branch has no receipt');
  await h.shutdown();
});

test('a notice held across a cancellation or /tree is delivered once, without a run', async () => {
  const fake = fakeDots();
  const review = fake.add('r-00000001', 'pi-s1');
  const t1 = fake.thread(review, 't1', null, ['user', 'Look here']);
  const h = runtime(fake.run);
  h.start();
  await settle();
  assert.equal(h.messages.length, 1);

  h.state.idle = false;
  fake.comment(t1, 'user', 'And here');
  h.changed();
  await settle();
  h.settled(true);
  await settle();
  assert.equal(h.messages.length, 2, 'recorded once');
  assert.deepEqual(
    h.messages[1].options,
    {triggerTurn: false},
    'the cancellation stands',
  );

  // A user prompt ends the cancellation; a notice held for the old leaf is
  // dropped, and the new branch is told once.
  h.handlers.before_agent_start({messageType: 'user', prompt: 'go on'}, h.ctx);
  h.state.idle = false;
  fake.comment(t1, 'user', 'Third');
  h.changed();
  await settle();
  h.branch.length = 0;
  h.handlers.session_tree({type: 'session_tree'}, h.ctx);
  await settle();
  h.settled();
  await settle();
  assert.equal(h.messages.length, 3);
  assert.deepEqual(h.messages[2].options, {triggerTurn: true});
  assert.deepEqual(h.messages[2].message.details.marks, {
    'r-00000001/t1': 3,
  });
  await h.shutdown();
});

test("other sessions' reviews and failures stay quiet", async () => {
  const fake = fakeDots();
  const theirs = fake.add('r-00000001', 'pi-other');
  fake.thread(theirs, 't1', null, ['user', 'Not for s1']);
  const h = runtime(fake.run);
  h.start();
  await settle();
  assert.equal(h.messages.length, 0);
  assert.equal(h.watches.size, 0);
  await h.shutdown();

  const outside = runtime(async () => {
    throw new Error('fatal: not a git repository (or any parent)');
  });
  outside.start();
  await settle();
  assert.deepEqual(outside.notices, []);
  await outside.shutdown();

  const broken = runtime(async () => {
    throw new Error('dots exploded');
  });
  broken.start();
  await settle();
  broken.handlers.agent_settled({type: 'agent_settled'}, broken.ctx);
  await settle();
  assert.deepEqual(broken.notices, [
    {message: 'Review notices unavailable: dots exploded', level: 'warning'},
  ]);
  await broken.shutdown();
});

test('the real CLI and file watcher deliver a reply in a linked worktree', async () => {
  const root = realpathSync(mkdtempSync(path.join(tmpdir(), 'dots-review-')));
  const env = {
    ...process.env,
    GIT_CONFIG_GLOBAL: '/dev/null',
    GIT_CONFIG_NOSYSTEM: '1',
    GIT_AUTHOR_NAME: 't',
    GIT_AUTHOR_EMAIL: 't@example.com',
    GIT_COMMITTER_NAME: 't',
    GIT_COMMITTER_EMAIL: 't@example.com',
  };
  delete env.PI_SESSION_ID;
  delete env.CLAUDE_CODE_SESSION_ID;
  delete env.CODEX_SESSION_ID;
  const checkout = path.join(root, 'checkout');
  const linked = path.join(root, 'linked');
  const run = (cwd, command, ...args) =>
    execFileSync(command, args, {cwd, env, encoding: 'utf8'});
  let h;
  try {
    run(root, 'git', 'init', '-q', '-b', 'main', checkout);
    writeFileSync(path.join(checkout, 'a.txt'), 'one\n');
    run(checkout, 'git', 'add', 'a.txt');
    run(checkout, 'git', 'commit', '-q', '-m', 'init');
    run(checkout, 'git', 'worktree', 'add', '-q', linked, '-b', 'slice');
    writeFileSync(path.join(linked, 'a.txt'), 'two\n');
    const started = JSON.parse(
      execFileSync('dots', ['--json', 'review', 'start', '--worktree'], {
        cwd: linked,
        env: {...env, PI_SESSION_ID: 'real'},
        encoding: 'utf8',
      }),
    );
    const id = started.review.id;
    assert.equal(started.review.owner, 'pi-real');
    run(
      linked,
      'dots',
      'review',
      'start',
      '--worktree',
      '--owner',
      'pi-someone-else',
    );

    const threads = path.join(root, 'threads.json');
    writeFileSync(
      threads,
      JSON.stringify({
        threads: [
          {
            anchor: {path: 'a.txt', side: 'new', start: 1, end: 1},
            body: 'A finding',
            author: 'reviewer',
          },
        ],
      }),
    );
    run(checkout, 'dots', 'review', 'add', id, '--threads', threads);

    h = runtime(undefined, {session: 'real', realWatch: true});
    h.ctx.cwd = checkout;
    h.start();
    await settle(1500);
    assert.equal(h.messages.length, 0);

    run(
      checkout,
      'dots',
      'review',
      'reply',
      id,
      't1',
      '--author',
      'user',
      '--body',
      'Please explain',
    );
    for (let waited = 0; h.messages.length === 0 && waited < 10_000;)
      waited += await settle(100).then(() => 100);
    assert.equal(h.messages.length, 1);
    assert.match(
      h.messages[0].message.content,
      new RegExp(
        `${id} in ${linked} \\(slice\\)\\n- t1 · a\\.txt:1 · user replied: Please explain`,
      ),
    );
  } finally {
    await h?.shutdown();
    rmSync(root, {recursive: true, force: true});
  }
});
