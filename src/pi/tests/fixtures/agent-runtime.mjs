// One Pi process with the real goal and jobs extensions loaded from source
// (re-imported on reload, as Pi loads them). Prints one JSON line per
// observation of what a model's bash command, a `!` command and a job see,
// then exits through Pi's own shutdown.
// Usage: agent-runtime.mjs lifecycle <cwd> <project> <task> <task>
//        agent-runtime.mjs observe <cwd> <session file>
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {existsSync, readFileSync} from 'node:fs';
import {setTimeout as delay} from 'node:timers/promises';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';

import {hostModule} from './host.ts';
const host = await import(hostModule('pi-coding-agent'));

const [flow, cwd, ...rest] = process.argv.slice(2);
const extensions = [
  fileURLToPath(new URL('../../extensions/goal/index.ts', import.meta.url)),
  fileURLToPath(new URL('../../extensions/jobs/index.ts', import.meta.url)),
  fileURLToPath(new URL('./agent-observer.mjs', import.meta.url)),
];
const agentDir = process.env.PI_CODING_AGENT_DIR;
assert.ok(agentDir, 'PI_CODING_AGENT_DIR must name a disposable directory');

// Provider-free: no model catalog, credentials or network; nothing prompts a model.
const modelRuntime = await host.ModelRuntime.create({
  authPath: join(agentDir, 'auth.json'),
  modelsPath: null,
  modelsStorePath: join(agentDir, 'models-store.json'),
  allowModelNetwork: false,
  refreshOnCreate: false,
});
assert.deepEqual(
  await modelRuntime.getAvailable(),
  [],
  'no model may have credentials',
);
const createRuntime = async ({cwd, sessionManager, sessionStartEvent}) => {
  const services = await host.createAgentSessionServices({
    cwd,
    agentDir,
    modelRuntime,
    settingsManager: host.SettingsManager.inMemory({
      compaction: {enabled: false},
      retry: {enabled: false},
      cacheWarming: 'off',
    }),
    resourceLoaderOptions: {
      additionalExtensionPaths: extensions,
      noExtensions: true,
      noSkills: true,
      noPromptTemplates: true,
      noThemes: true,
      noContextFiles: true,
    },
  });
  const errors = services.resourceLoader.getExtensions().errors;
  assert.deepEqual(errors, []);
  const created = await host.createAgentSessionFromServices({
    services,
    sessionManager,
    sessionStartEvent,
    noTools: 'builtin',
  });
  return {...created, services, diagnostics: services.diagnostics};
};

const failures = [];
const bind = () =>
  runtime.session.bindExtensions({
    mode: 'rpc',
    onError: error => failures.push(String(error.error ?? error)),
  });
const PRINT = `printf '%s\\n%s' "$PI_SESSION_ID" "$DOTS_AGENT_OWNER"`;
const parse = text => {
  const [session = '', owner = ''] = text.trim().split('\n');
  return {session, owner};
};
/** What a model's bash command, a `!` command and a job see. */
async function observe(step, claim) {
  const session = runtime.session;
  await session.prompt(
    `/fixture-agent ${step}${claim ? ` ${claim} ${project}` : ''}`,
  );
  const report = globalThis.fixtureAgentReport;
  assert.equal(report?.step, step);
  const user = await session.executeBash(PRINT);
  assert.equal(user.exitCode, 0);
  const file = join(agentDir, `job-${process.pid}-${step}.txt`);
  const runner = session.extensionRunner;
  await runner
    .getToolDefinition('job_start')
    .execute(
      'fixture-job',
      {command: `${PRINT} > '${file}.tmp' && mv '${file}.tmp' '${file}'`},
      undefined,
      undefined,
      runner.createToolContext('fixture-job', undefined),
    );
  for (let waited = 0; !existsSync(file); waited += 50) {
    assert.ok(waited < 10_000, `job for ${step} did not finish`);
    await delay(50);
  }
  console.log(
    JSON.stringify({
      step,
      session: session.sessionManager.getSessionId(),
      shell: report.shell,
      user: parse(user.output),
      job: parse(readFileSync(file, 'utf8')),
    }),
  );
}
const owner = task =>
  JSON.parse(
    execFileSync(
      'dots',
      ['--json', 'task', 'show', task, '--project', project],
      {
        encoding: 'utf8',
      },
    ),
  ).task.owner;
const report = (step, value) => console.log(JSON.stringify({step, ...value}));

let project;
let runtime;
const sessions = join(agentDir, 'sessions');
if (flow === 'observe') {
  const [file] = rest;
  runtime = await host.createAgentSessionRuntime(createRuntime, {
    cwd,
    agentDir,
    sessionManager: host.SessionManager.open(file, sessions),
  });
  await bind();
  await observe('observe');
} else if (flow === 'lifecycle') {
  const [selected, first, second] = rest;
  project = selected;
  runtime = await host.createAgentSessionRuntime(createRuntime, {
    cwd,
    agentDir,
    sessionManager: host.SessionManager.create(cwd, sessions),
  });
  await bind();
  await observe('startup', first);
  // Persist the session so it can be resumed and cloned.
  runtime.session.sessionManager.appendMessage({
    role: 'user',
    content: 'Agent identity fixture',
    timestamp: Date.now(),
  });
  const file = runtime.session.sessionFile;
  report('file', {file});
  await runtime.session.reload();
  await observe('reload');
  report('after-reload', {owner: owner(first)});
  const entries = runtime.session.sessionManager.getEntries();
  runtime.session.sessionManager.appendCustomEntry('fixture.branch', {});
  await runtime.session.navigateTree(entries.at(-1).id);
  await observe('tree');
  await runtime.newSession();
  await bind();
  await observe('new', second);
  report('after-new', {owner: owner(first)});
  await runtime.switchSession(file);
  await bind();
  await observe('resume');
  report('after-resume', {owner: owner(second)});
  const message = runtime.session.sessionManager
    .getEntries()
    .find(entry => entry.type === 'message');
  await runtime.fork(message.id, {position: 'at'});
  await bind();
  await observe('fork');
} else throw new Error(`unknown flow: ${flow}`);
await runtime.dispose();
assert.deepEqual(failures, []);
