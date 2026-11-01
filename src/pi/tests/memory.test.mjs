import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import register, {
  decodeScopes,
  decodeIndex,
  loadIndex,
  memoryContext,
  memoryScopes,
} from '../extensions/memory/index.ts';

function scratchRepo() {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-memory-'));
  const checkout = path.join(root, 'checkout');
  mkdirSync(checkout);
  const git = (cwd, ...args) =>
    execFileSync('git', args, {
      cwd,
      stdio: 'ignore',
      env: {
        ...process.env,
        GIT_CONFIG_GLOBAL: '/dev/null',
        GIT_CONFIG_NOSYSTEM: '1',
        GIT_AUTHOR_NAME: 't',
        GIT_AUTHOR_EMAIL: 't@example.com',
        GIT_COMMITTER_NAME: 't',
        GIT_COMMITTER_EMAIL: 't@example.com',
      },
    });
  git(checkout, 'init', '-q', '-b', 'main');
  writeFileSync(path.join(checkout, 'README.md'), 'x\n');
  git(checkout, 'add', 'README.md');
  git(checkout, 'commit', '-q', '-m', 'init');
  git(
    checkout,
    'worktree',
    'add',
    '-q',
    path.join(root, 'linked'),
    '-b',
    'slice',
  );
  execFileSync(
    'dots',
    ['--directory', checkout, 'project', 'init', 'Fixture', '--fresh'],
    {
      env: environment(root),
      stdio: 'ignore',
    },
  );
  return {
    root,
    checkout,
    linked: path.join(root, 'linked'),
    sub: path.join(checkout, 'nested'),
  };
}

async function memoryText(cwd, env, signal) {
  return {
    content: Object.values(await memoryContext(cwd, env, signal)).join('\n\n'),
  };
}
function environment(root) {
  return {
    ...process.env,
    HOME: root,
    XDG_DATA_HOME: path.join(root, 'data'),
    XDG_STATE_HOME: path.join(root, 'state'),
  };
}
function indexFile(root) {
  return path.join(root, 'data/dots/workspaces/global/knowledge/MEMORY.md');
}
function put(file, content) {
  mkdirSync(path.dirname(file), {recursive: true});
  writeFileSync(file, content);
}

test('real CLI resolves worktrees, subdirectories and non-Git with one authoritative scope', async () => {
  const repo = scratchRepo();
  mkdirSync(repo.sub);
  try {
    const env = environment(repo.root);
    const main = await memoryScopes(repo.checkout, {env});
    assert.deepEqual(await memoryScopes(repo.linked, {env}), main);
    assert.deepEqual(await memoryScopes(repo.sub, {env}), main);
    const alias = path.join(repo.root, 'alias');
    symlinkSync(repo.checkout, alias);
    assert.deepEqual(await memoryScopes(alias, {env}), main);
    assert.ok(
      main.project.startsWith(path.join(repo.root, 'data/dots/workspaces')),
    );
    assert.equal((await memoryScopes(repo.root, {env})).project, null);
  } finally {
    rmSync(repo.root, {recursive: true, force: true});
  }
});

test('fresh global/project indexes retain revisions, missing pointers and independent sections', async () => {
  const repo = scratchRepo();
  try {
    const env = environment(repo.root);
    const scopes = await memoryScopes(repo.checkout, {env});
    const projectFile = path.join(scopes.project, 'MEMORY.md');
    const fresh = await memoryText(repo.linked, env);
    assert.doesNotMatch(fresh.content, /Today is|2026-10-15/);
    assert.ok(
      fresh.content.includes(indexFile(repo.root)) &&
        fresh.content.includes(projectFile),
    );
    assert.equal(fresh.content.match(/Not created yet/g).length, 2);
    assert.equal(fresh.content.match(/Memory conventions:/g).length, 1);
    const absent = await loadIndex(repo.checkout, 'global', {env});
    assert.equal(absent.exists, false);
    assert.equal(absent.revision, 'missing');
    put(indexFile(repo.root), '# Global memory\nGlobal fact\n');
    put(projectFile, '# Project memory\nProject fact\n');
    const first = await loadIndex(repo.checkout, 'global', {env});
    assert.equal(first.exists, true);
    assert.notEqual(first.revision, 'missing');
    const loaded = await memoryText(repo.linked, env);
    assert.match(loaded.content, /Global fact[\s\S]*Project fact/);
    put(indexFile(repo.root), '');
    const empty = await loadIndex(repo.checkout, 'global', {env});
    assert.equal(empty.exists, true);
    assert.notEqual(empty.revision, first.revision);
    assert.doesNotMatch(
      (await memoryText(repo.linked, env)).content,
      /Global fact|Not created yet/,
    );

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
    const sections = {};
    for (let n = 0; n < 2; n++)
      await handlers.before_agent_start(
        {systemPromptOptions: {sections}},
        {cwd: repo.linked},
      );
    assert.equal(Object.keys(sections).length, 2);
    const before = {...sections};
    put(projectFile, 'Corrected fact');
    await handlers.before_agent_start(
      {systemPromptOptions: {sections}},
      {cwd: repo.linked},
    );
    assert.equal(Object.keys(sections).length, 2);
    assert.equal(sections.dots_memory_global, before.dots_memory_global);
    assert.match(sections.dots_memory_project, /Corrected fact/);
    assert.doesNotMatch(sections.dots_memory_project, /Project fact/);
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

test('reads only indexes and accepts Rust page limits without cutting UTF-8', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-memory-page-'));
  try {
    const env = environment(root);
    const file = indexFile(root);
    put(file, Array.from({length: 300}, (_, i) => `- line ${i}`).join('\n'));
    put(path.join(path.dirname(file), 'wiki/topic.md'), 'DETAIL_ONLY');
    put(
      path.join(
        root,
        'data/dots/workspaces/p-00000000000000000000000000000002/knowledge/MEMORY.md',
      ),
      'OTHER_PROJECT_ONLY',
    );
    const page = await loadIndex(root, 'global', {env});
    assert.equal(page.next_line, 201);
    assert.match(page.content, /line 199/);
    assert.doesNotMatch(page.content, /line 200/);
    assert.match(
      (await memoryText(root, env)).content,
      /truncated; use dots memory read/,
    );
    put(file, Array(40).fill('界'.repeat(300)).join('\n'));
    const unicode = await loadIndex(root, 'global', {env});
    assert.ok(Buffer.byteLength(unicode.content) <= 16 * 1024);
    assert.doesNotMatch(unicode.content, /\ufffd/);
    assert.ok(unicode.next_line > 1);
    assert.doesNotMatch(
      (await memoryText(root, env)).content,
      /DETAIL_ONLY|OTHER_PROJECT_ONLY/,
    );
  } finally {
    rmSync(root, {recursive: true, force: true});
  }
});

test('oversized input, long lines, directories, FIFOs and symlinks are unavailable, never missing', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-memory-unavailable-'));
  try {
    const env = environment(root);
    const file = indexFile(root);
    for (const content of [
      'x'.repeat(256 * 1024 + 1),
      'x'.repeat(16 * 1024 + 1),
    ]) {
      put(file, content);
      const block = await memoryText(root, env);
      assert.match(block.content, /Index unavailable/);
      assert.doesNotMatch(block.content, /Not created yet/);
    }
    if (process.getuid?.() !== 0 && process.platform !== 'win32') {
      put(file, 'Unreadable index');
      chmodSync(file, 0);
      try {
        assert.match(
          (await memoryText(root, env)).content,
          /Index unavailable/,
        );
      } finally {
        chmodSync(file, 0o600);
      }
    }
    rmSync(file);
    mkdirSync(file);
    assert.match((await memoryText(root, env)).content, /not a regular file/);
    rmSync(file, {recursive: true});
    if (process.platform !== 'win32') {
      execFileSync('mkfifo', [file]);
      assert.match((await memoryText(root, env)).content, /not a regular file/);
      rmSync(file);
    }
    symlinkSync(path.join(root, 'missing'), file);
    assert.match((await memoryText(root, env)).content, /symlink/);
  } finally {
    rmSync(root, {recursive: true, force: true});
  }
});

test('failed project discovery preserves global memory without claiming to be outside Git', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-memory-git-'));
  try {
    execFileSync('git', ['init', '--quiet', root]);
    execFileSync(
      'dots',
      ['--directory', root, 'project', 'init', 'Fixture', '--fresh'],
      {env: environment(root), stdio: 'ignore'},
    );
    put(indexFile(root), 'GLOBAL_FACT survives project discovery failure');
    const bin = path.join(root, 'bin');
    mkdirSync(bin);
    symlinkSync(
      realpathSync(execFileSync('which', ['dots'], {encoding: 'utf8'}).trim()),
      path.join(bin, 'dots'),
    );
    const block = await memoryText(root, {...environment(root), PATH: bin});
    assert.match(
      block.content,
      /GLOBAL_FACT[\s\S]*Project memory[\s\S]*scope unavailable/,
    );
    assert.doesNotMatch(
      block.content,
      /Not inside a Git repository|Not created yet/,
    );
    rmSync(path.join(root, '.git'), {recursive: true});
    put(path.join(root, '.git'), 'gitdir: /missing-fixture-checkout');
    const broken = await memoryText(root, environment(root));
    assert.match(
      broken.content,
      /GLOBAL_FACT[\s\S]*Project memory[\s\S]*scope unavailable/,
    );
    assert.doesNotMatch(broken.content, /Not inside a Git repository/);
  } finally {
    rmSync(root, {recursive: true, force: true});
  }
});

test('a deleted session directory does not suppress global memory', async () => {
  const root = mkdtempSync(path.join(tmpdir(), 'dots-memory-deleted-'));
  try {
    const cwd = path.join(root, 'session');
    mkdirSync(cwd);
    put(indexFile(root), 'GLOBAL_FACT survives a deleted session directory');
    rmSync(cwd, {recursive: true});
    const block = await memoryText(cwd, environment(root));
    assert.match(
      block.content,
      /GLOBAL_FACT[\s\S]*Project memory[\s\S]*scope unavailable/,
    );
    assert.doesNotMatch(
      block.content,
      /Not inside a Git repository|Not created yet/,
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

test('memory decoders reject malformed external fields', () => {
  for (const value of [
    null,
    {},
    {global: 'relative', project: null},
    {global: '/global'},
  ])
    assert.throws(() => decodeScopes(value));
  const valid = {
    scope: 'global',
    document: {
      path: '/global/MEMORY.md',
      exists: false,
      revision: 'missing',
      content: '',
      next_line: null,
    },
  };
  assert.equal(decodeIndex(valid, 'global').exists, false);
  for (const fields of [
    {exists: 'false'},
    {revision: 'hash'},
    {next_line: 0},
    {content: null},
  ]) {
    assert.throws(() =>
      decodeIndex(
        {...valid, document: {...valid.document, ...fields}},
        'global',
      ),
    );
  }
  assert.throws(() => decodeIndex(valid, 'project'));
});
