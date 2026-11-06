import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import register, {
  decodeMemorySnapshot,
  memorySnapshot,
  memoryContext,
} from '../extensions/memory/index.ts';

function environment(root) {
  return {
    PATH: process.env.PATH,
    HOME: root,
    XDG_DATA_HOME: path.join(root, 'data'),
    XDG_STATE_HOME: path.join(root, 'state'),
  };
}
function cli(root, cwd, ...args) {
  return JSON.parse(
    execFileSync('dots', ['--json', '--directory', cwd, ...args], {
      env: environment(root),
      encoding: 'utf8',
    }),
  );
}
function publish(repo, changes) {
  const current = cli(repo.root, repo.checkout, 'memory', 'status');
  const input = path.join(repo.root, 'changes.json');
  writeFileSync(input, JSON.stringify(changes));
  return cli(
    repo.root,
    repo.checkout,
    'memory',
    'publish',
    '--expected-snapshot',
    current.snapshot,
    '--input-file',
    input,
  );
}
function write(scope, content, relative = 'MEMORY.md') {
  return {scope, path: relative, action: {kind: 'write', content}};
}
function scratchRepo() {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-knowledge-'));
  const checkout = path.join(root, 'checkout');
  mkdirSync(checkout);
  const git = (...args) =>
    execFileSync('git', args, {
      cwd: checkout,
      stdio: 'ignore',
      env: {
        PATH: process.env.PATH,
        HOME: root,
        GIT_CONFIG_GLOBAL: '/dev/null',
        GIT_CONFIG_NOSYSTEM: '1',
        GIT_AUTHOR_NAME: 'fixture',
        GIT_AUTHOR_EMAIL: 'fixture@example.com',
        GIT_COMMITTER_NAME: 'fixture',
        GIT_COMMITTER_EMAIL: 'fixture@example.com',
      },
    });
  git('init', '-q', '-b', 'main');
  git(
    '-c',
    'commit.gpgsign=false',
    'commit',
    '--allow-empty',
    '-qm',
    'fixture',
  );
  git('worktree', 'add', '-qb', 'slice', path.join(root, 'linked'));
  cli(root, checkout, 'project', 'init', 'Fixture', '--fresh');
  const initialized = cli(
    root,
    checkout,
    'memory',
    'init',
    '--actor',
    'fixture',
  );
  const registration = cli(
    root,
    checkout,
    'memory',
    'project',
    'create',
    'Fixture',
    '--expected-snapshot',
    initialized.snapshot,
  );
  cli(root, checkout, 'memory', 'project', 'bind', registration.project);
  return {
    root,
    checkout,
    linked: path.join(root, 'linked'),
    project: {kind: 'project', project: registration.project},
  };
}
async function memoryText(cwd, env, signal) {
  return {
    content: Object.values(await memoryContext(cwd, env, signal)).join('\n\n'),
  };
}

test('real CLI uses stable shared identities for worktrees, subdirectories and aliases', async () => {
  const repo = scratchRepo();
  try {
    const env = environment(repo.root);
    const main = await memorySnapshot(repo.checkout, {env});
    assert.deepEqual(await memorySnapshot(repo.linked, {env}), main);
    const sub = path.join(repo.checkout, 'nested');
    mkdirSync(sub);
    assert.deepEqual(await memorySnapshot(sub, {env}), main);
    const alias = path.join(repo.root, 'alias');
    symlinkSync(repo.checkout, alias);
    assert.deepEqual(await memorySnapshot(alias, {env}), main);
    assert.equal(main.project.scope, repo.project.project);
    assert.equal((await memorySnapshot(repo.root, {env})).project, null);
  } finally {
    rmSync(repo.root, {recursive: true, force: true});
  }
});

test('indexes share one snapshot, retain missing/empty states and replace native sections', async () => {
  const repo = scratchRepo();
  try {
    const env = environment(repo.root);
    const fresh = await memoryText(repo.linked, env);
    assert.equal(fresh.content.match(/Not created yet/g).length, 2);
    assert.equal(fresh.content.match(/Memory conventions:/g).length, 1);
    assert.doesNotMatch(fresh.content, /workspaces\/|Index: \//);
    const publication = publish(repo, [
      write({kind: 'global'}, 'Global fact'),
      write(repo.project, 'Project fact'),
    ]);
    const loaded = await memorySnapshot(repo.linked, {env});
    assert.equal(loaded.global.snapshot, publication.snapshot);
    assert.equal(loaded.project.snapshot, publication.snapshot);
    assert.match(
      (await memoryText(repo.linked, env)).content,
      /Global fact[\s\S]*Project fact/,
    );
    publish(repo, [write({kind: 'global'}, '')]);
    const empty = await memorySnapshot(repo.checkout, {env});
    assert.equal(empty.global.exists, true);
    assert.equal(empty.global.content, '');
    const handlers = {};
    register(
      {
        on: (name, fn) => {
          handlers[name] = fn;
        },
        registerCommand() {},
      },
      (cwd, _env, signal) => memoryContext(cwd, env, signal),
    );
    const sections = {other_source: 'unrelated'};
    for (let n = 0; n < 2; n++)
      await handlers.before_agent_start(
        {systemPromptOptions: {sections}},
        {cwd: repo.linked},
      );
    assert.equal(Object.keys(sections).length, 3);
    const changed = publish(repo, [write(repo.project, 'Corrected fact')]);
    await handlers.before_agent_start(
      {systemPromptOptions: {sections}},
      {cwd: repo.linked},
    );
    assert.match(sections.dots_memory_project, /Corrected fact/);
    assert.doesNotMatch(sections.dots_memory_project, /Project fact/);
    assert.ok(sections.dots_memory_global.includes(changed.snapshot));
    assert.equal(sections.other_source, 'unrelated');
    await handlers.before_agent_start(
      {systemPromptOptions: {sections}},
      {cwd: repo.root},
    );
    assert.match(sections.dots_memory_project, /Project scope unavailable/);
    assert.doesNotMatch(sections.dots_memory_project, /Corrected fact/);
  } finally {
    rmSync(repo.root, {recursive: true, force: true});
  }
});

test('pages carry pinned retrieval commands, bounded Unicode and no unrelated topics', async () => {
  const repo = scratchRepo();
  try {
    const env = environment(repo.root);
    publish(repo, [
      write(
        {kind: 'global'},
        Array.from({length: 300}, (_, i) => `- line ${i}`).join('\n'),
      ),
      write({kind: 'global'}, 'DETAIL_ONLY', 'wiki/topic.md'),
    ]);
    const page = await memorySnapshot(repo.checkout, {env});
    assert.equal(page.global.nextLine, 201);
    assert.match(page.global.content, /line 199/);
    assert.doesNotMatch(page.global.content, /line 200/);
    const text = (await memoryText(repo.checkout, env)).content;
    assert.match(text, /truncated; use dots memory read/);
    assert.ok(text.includes(`--snapshot ${page.snapshot} --start 201`));
    assert.doesNotMatch(text, /DETAIL_ONLY/);
    publish(repo, [
      write({kind: 'global'}, Array(40).fill('界'.repeat(300)).join('\n')),
    ]);
    const unicode = await memorySnapshot(repo.checkout, {env});
    assert.ok(Buffer.byteLength(unicode.global.content) <= 16 * 1024);
    assert.doesNotMatch(unicode.global.content, /\ufffd/);
    assert.ok(unicode.global.nextLine > 1);
  } finally {
    rmSync(repo.root, {recursive: true, force: true});
  }
});

test('unavailable index pages preserve the other scope without claiming absence', async () => {
  const repo = scratchRepo();
  try {
    const env = environment(repo.root);
    publish(repo, [
      write({kind: 'global'}, 'x'.repeat(16 * 1024 + 1)),
      write(repo.project, 'PROJECT_FACT'),
    ]);
    const text = (await memoryText(repo.checkout, env)).content;
    assert.match(
      text,
      /Global memory[\s\S]*Index unavailable[\s\S]*PROJECT_FACT/,
    );
    assert.doesNotMatch(text, /Not created yet/);
    publish(repo, [
      write({kind: 'global'}, 'GLOBAL_FACT'),
      write(repo.project, 'x'.repeat(16 * 1024 + 1)),
    ]);
    assert.match(
      (await memoryText(repo.checkout, env)).content,
      /GLOBAL_FACT[\s\S]*Project memory[\s\S]*Index unavailable/,
    );
  } finally {
    rmSync(repo.root, {recursive: true, force: true});
  }
});

test('project lookup failure and deleted cwd preserve globally published knowledge', async () => {
  const repo = scratchRepo();
  try {
    const env = environment(repo.root);
    publish(repo, [write({kind: 'global'}, 'GLOBAL_FACT')]);
    const gone = path.join(repo.root, 'gone');
    mkdirSync(gone);
    rmSync(gone, {recursive: true});
    const missing = (await memoryText(gone, env)).content;
    assert.match(missing, /GLOBAL_FACT[\s\S]*Project scope unavailable/);
    assert.doesNotMatch(missing, /Not created yet/);
    rmSync(path.join(repo.checkout, '.git'), {recursive: true});
    writeFileSync(
      path.join(repo.checkout, '.git'),
      'gitdir: /missing-neutral-checkout',
    );
    assert.match(
      (await memoryText(repo.checkout, env)).content,
      /GLOBAL_FACT[\s\S]*Project scope unavailable/,
    );
  } finally {
    rmSync(repo.root, {recursive: true, force: true});
  }
});

test('uninitialized or corrupt knowledge never falls back to filesystem indexes', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-unavailable-'));
  try {
    const loose = path.join(root, 'loose-documents');
    mkdirSync(loose, {recursive: true});
    writeFileSync(path.join(loose, 'MEMORY.md'), 'UNACCEPTED_FILESYSTEM_FACT');
    const text = (await memoryText(root, environment(root))).content;
    assert.match(text, /Index unavailable/);
    assert.doesNotMatch(text, /Not created yet|UNACCEPTED_FILESYSTEM_FACT/);
    cli(root, root, 'memory', 'init', '--actor', 'fixture');
    writeFileSync(
      path.join(root, 'data/dots/knowledge/store.json'),
      '{invalid',
    );
    assert.match(
      (await memoryText(root, environment(root))).content,
      /Index unavailable/,
    );
  } finally {
    rmSync(root, {recursive: true, force: true});
  }
});

test('session shutdown and replacement cannot inject or notify stale context', async () => {
  for (const end of ['shutdown', 'start']) {
    const handlers = {};
    let command;
    let finish;
    let observedSignal;
    const pending = new Promise(resolve => {
      finish = resolve;
    });
    register(
      {
        on: (name, fn) => {
          handlers[name] = fn;
        },
        registerCommand: (_name, value) => {
          command = value;
        },
      },
      async (_cwd, _env, signal) => {
        observedSignal = signal;
        return pending;
      },
    );
    const sections = {};
    const work = handlers.before_agent_start(
      {systemPromptOptions: {sections}},
      {cwd: '/fixture', signal: undefined},
    );
    await handlers[`session_${end}`]();
    assert.equal(observedSignal.aborted, true);
    finish({dots_memory_global: 'STALE'});
    await work;
    assert.deepEqual(sections, {});
    if (end === 'shutdown') {
      let notified = false;
      await command.handler('', {
        hasUI: true,
        cwd: '/fixture',
        ui: {
          notify() {
            notified = true;
          },
        },
      });
      assert.equal(notified, false);
    }
  }
  const cause = new Error('already cancelled');
  await assert.rejects(
    memoryContext('/missing', {}, AbortSignal.abort(cause)),
    error => error === cause,
  );
});

test('memory snapshot decoder rejects cross-snapshot, wrong-scope and filesystem resource responses', () => {
  const snapshot = 'a'.repeat(64);
  const value = {
    identity: {
      store: 'k-' + '0'.repeat(32),
      node: 'n-' + '1'.repeat(32),
      genesis: snapshot,
      role: 'authority',
    },
    snapshot,
    project: null,
    diagnostic: 'unbound',
    global: {
      state: 'available',
      snapshot,
      scope: {kind: 'global'},
      document: {
        path: 'MEMORY.md',
        exists: false,
        content: '',
        revision: 'missing',
        next_line: null,
      },
    },
  };
  assert.equal(decodeMemorySnapshot(value).global.exists, false);
  for (const malformed of [
    null,
    {},
    {...value, snapshot: 'short'},
    {...value, global: {...value.global, snapshot: 'b'.repeat(64)}},
    {
      ...value,
      global: {
        ...value.global,
        scope: {kind: 'project', project: 'p-' + '2'.repeat(32)},
      },
    },
    {
      ...value,
      global: {
        ...value.global,
        document: {...value.global.document, path: '/fake/MEMORY.md'},
      },
    },
    {
      ...value,
      global: {
        ...value.global,
        document: {...value.global.document, content: 'not absent'},
      },
    },
    {...value, global: {...value.global, state: 'incoming'}},
    {...value, diagnostic: null},
  ]) {
    assert.throws(() => decodeMemorySnapshot(malformed));
  }
});
