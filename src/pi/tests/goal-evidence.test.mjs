import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {mkdtempSync, mkdirSync, writeFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import {
  decodeEvidence,
  decodeOutputPage,
  dotsEvidenceSource,
} from '../extensions/goal/source.ts';
import {
  evidenceArgs,
  evidenceRecordText,
  inspectEvidence,
} from '../extensions/goal/evidence.ts';

const selection = {slug: 'report', digest: 'sha256:' + 'a'.repeat(64)};
test('evidence command and wire identities fail closed', () => {
  assert.deepEqual(evidenceArgs(['t-123abc', 'current', 'p-example']), {
    id: 't-123abc',
    project: 'p-example',
  });
  for (const args of [
    [],
    ['oops'],
    ['t-123abc', '0'],
    ['t-123abc', '1.5'],
    ['t-123abc', '9007199254740992'],
  ])
    assert.throws(() => evidenceArgs(args));
  assert.deepEqual(decodeEvidence(selection), selection);
  for (const change of [
    {slug: '../report'},
    {slug: '-report'},
    {digest: 'latest'},
    {digest: 'sha256:a'},
  ])
    assert.throws(() => decodeEvidence({...selection, ...change}));
  const wire = {
    project: 'p-example',
    output: {
      revision: selection,
      path: '/example/report.md',
      content: {
        state: 'available',
        document: {
          exists: true,
          revision: selection.digest,
          path: '/example/report.md',
          start: 1,
          total_lines: 3,
          next_line: 3,
          content: '# Report\n',
        },
      },
    },
  };
  assert.equal(decodeOutputPage(wire, 'p-example', selection, 1).next, 3);
  assert.throws(() => decodeOutputPage(wire, 'p-other', selection, 1));
  assert.throws(() =>
    decodeOutputPage(wire, 'p-example', {...selection, slug: 'other'}, 1),
  );
  assert.throws(() => decodeOutputPage(wire, 'p-example', selection, 2));
  for (const state of ['missing', 'corrupt', 'purged']) {
    const value = structuredClone(wire);
    value.output.content = {state};
    assert.equal(
      decodeOutputPage(value, 'p-example', selection, 1).state,
      state,
    );
  }
  wire.output.content.document.next_line = 1;
  assert.throws(() => decodeOutputPage(wire, 'p-example', selection, 1));
});

test('cold task-only inspection pins historical bytes and reports purge without changing tasks', async () => {
  const root = mkdtempSync(join(tmpdir(), 'dots-evidence-'));
  const keys = [
    'HOME',
    'XDG_DATA_HOME',
    'XDG_CONFIG_HOME',
    'XDG_STATE_HOME',
    'DOTS_MEMORY_ROOT',
  ];
  const prior = keys.map(key => process.env[key]);
  for (const key of keys) process.env[key] = join(root, key);
  const signal = new AbortController().signal;
  const cli = (...args) =>
    JSON.parse(
      execFileSync('dots', ['--directory', root, '--json', ...args], {
        encoding: 'utf8',
      }),
    );
  try {
    mkdirSync(process.env.HOME, {recursive: true});
    execFileSync('git', ['init', '--quiet', root]);
    const project = cli('project', 'init', 'Neutral evidence').project.id;
    const id = cli('task', 'add', 'Task without a goal').task.id;
    const draft = join(root, 'report.md');
    writeFileSync(draft, '# Exact evidence\n' + 'Recorded line\n'.repeat(220));
    const published = cli(
      'output',
      'publish',
      'report',
      '--create',
      '--task',
      id,
      '--filename',
      'report.md',
      '--media-type',
      'text/markdown',
      '--content-file',
      draft,
    ).publication.revision;
    const accepted = cli(
      'task',
      'update',
      id,
      '--result',
      'Reviewed first revision',
      '--evidence',
      'report',
      published.digest,
    );
    const seq = accepted.event.seq;
    writeFileSync(draft, '# Changed draft');
    cli(
      'output',
      'publish',
      'report',
      '--filename',
      'report.md',
      '--media-type',
      'text/markdown',
      '--content-file',
      draft,
    );
    const attached = await dotsEvidenceSource.attached(
      root,
      project,
      id,
      0,
      signal,
    );
    assert.deepEqual(attached, {slugs: ['report'], next: null});
    const manifest = await dotsEvidenceSource.manifest(
      root,
      project,
      'report',
      0,
      signal,
    );
    assert.equal(manifest.revisions.length, 2);
    assert.ok(
      manifest.revisions.some(revision => revision.digest === published.digest),
    );
    assert.equal(manifest.next, null);
    const record = await dotsEvidenceSource.record(root, id, signal);
    assert.equal(record.seq, seq);
    assert.deepEqual(record.evidence, [
      {slug: 'report', digest: published.digest},
    ]);
    const page = await dotsEvidenceSource.page(
      root,
      project,
      record.evidence[0],
      1,
      signal,
    );
    assert.match(page.text, /Exact evidence/);
    assert.equal(page.next, 201);
    const next = await dotsEvidenceSource.page(
      root,
      project,
      record.evidence[0],
      page.next,
      signal,
    );
    assert.equal(next.next, null);
    cli('task', 'update', id, '--result', 'Later result without evidence');
    const history = await dotsEvidenceSource.record(
      root,
      id,
      signal,
      seq,
      project,
    );
    assert.notEqual(history.current, history.seq);
    assert.equal(history.result, 'Reviewed first revision');
    const before = cli('task', 'show', id);
    cli('output', 'purge', '--digest', published.digest);
    const purged = await dotsEvidenceSource.record(
      root,
      id,
      signal,
      seq,
      project,
    );
    assert.match(evidenceRecordText(purged), /purged; unavailable/);
    assert.equal(
      (
        await dotsEvidenceSource.page(
          root,
          project,
          purged.evidence[0],
          1,
          signal,
        )
      ).state,
      'purged',
    );
    const after = cli('task', 'show', id);
    assert.deepEqual(after.task, before.task);
    assert.deepEqual(after.events, before.events);
    assert.equal(after.project, before.project);
  } finally {
    keys.forEach((key, i) => {
      if (prior[i] === undefined) delete process.env[key];
      else process.env[key] = prior[i];
    });
    rmSync(root, {recursive: true, force: true});
  }
});

test('discovery pages manifests explicitly and pages exact bytes without accepting them', async () => {
  const {initTheme} = await import('@earendil-works/pi-coding-agent');
  const {getThemeByName} =
    await import('../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js');
  initTheme('dark', false);
  const theme = getThemeByName('dark');
  const calls = [];
  const record = {
    project: 'p-example',
    task: 't-123abc',
    seq: null,
    current: null,
    result: null,
    evidence: [],
    purged: [],
  };
  const source = {
    async attached(_cwd, project, id, offset) {
      calls.push(['attached', project, id, offset]);
      return offset === 0
        ? {slugs: [], next: 50}
        : {slugs: ['report'], next: null};
    },
    async manifest(_cwd, project, slug, offset) {
      calls.push(['manifest', project, slug, offset]);
      return offset === 0
        ? {revisions: [], next: 20}
        : {
            revisions: [
              {
                ...selection,
                description: 'Neutral report',
                provenance: 'Fixture author',
                purged: false,
              },
            ],
            next: null,
          };
    },
    async page(_cwd, project, evidence, start) {
      calls.push(['page', project, evidence.digest, start]);
      return {
        state: 'available',
        path: '/fixture/report.md',
        text: '# Exact page',
        next: start === 1 ? 201 : null,
      };
    },
  };
  const choices = [
    'Attached outputs (not acceptance)',
    'More outputs…',
    'report',
    'More revisions…',
    selection.digest,
  ];
  const actions = ['n', 'b', undefined];
  await inspectEvidence(
    {
      cwd: '/fixture',
      ui: {
        async select(_title, options) {
          const choice = choices.shift();
          if (choice !== undefined) assert.ok(options.includes(choice));
          return choice;
        },
        async custom(factory) {
          return new Promise(resolve => {
            const done = resolve;
            const view = factory(
              {requestRender() {}, terminal: {rows: 40}},
              theme,
              {matches: () => false},
              done,
            );
            const rendered = view.render(100).join('\n');
            assert.match(rendered, /not accepted evidence/);
            const action = actions.shift();
            if (action) view.handleInput(action);
            else done('close');
            view.dispose();
          });
        },
      },
    },
    record,
    source,
    new AbortController().signal,
  );
  assert.deepEqual(
    calls.filter(call => call[0] === 'page').map(call => call[3]),
    [1, 201, 1],
  );
  assert.deepEqual(
    calls.filter(call => call[0] === 'manifest').map(call => call[3]),
    [0, 20, 20],
  );
});

test('evidence picker cancellation does not read bytes; abort dismisses a live inspector', async () => {
  const controller = new AbortController();
  const record = {
    project: 'p-example',
    task: 't-123abc',
    seq: 4,
    current: 4,
    result: 'Reviewed',
    evidence: [selection],
    purged: [],
  };
  let calls = 0;
  const source = {
    async page() {
      calls++;
      throw new Error('Unexpected read');
    },
  };
  await inspectEvidence(
    {
      ui: {
        async select() {
          return undefined;
        },
      },
    },
    record,
    source,
    controller.signal,
  );
  assert.equal(calls, 0);
  const {initTheme} = await import('@earendil-works/pi-coding-agent');
  const {getThemeByName} =
    await import('../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js');
  initTheme('dark', false);
  const theme = getThemeByName('dark');
  await inspectEvidence(
    {
      ui: {
        async select() {
          return 'Result record';
        },
        async custom(factory) {
          return new Promise(resolve => {
            const done = resolve;
            const view = factory(
              {requestRender() {}, terminal: {rows: 40}},
              theme,
              {matches: () => false},
              done,
            );
            assert.ok(view.render(100).length);
            controller.abort();
            view.dispose();
          });
        },
      },
    },
    record,
    source,
    controller.signal,
  );
  assert.equal(calls, 0);
});
