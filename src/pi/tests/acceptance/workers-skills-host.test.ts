// Explicit skill handoff through the pinned Pi host: the parent is Pi's own
// SDK session with real skill discovery (plus one SDK-injected virtual skill)
// and a scripted model; every child is the installed `pi` CLI with a
// deterministic provider, a different cwd, and real session files, reload and
// continuation. Selection follows the run's prompt-option skills: it honours
// `disable-model-invocation` and an earlier extension's omission. Selected
// skill files reach the child's first request in full, read at each launch,
// with no read call; an unreadable one fails the launch and leaves no worker.
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import {tmpdir} from 'node:os';
import {dirname, join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import type {
  AssistantMessage,
  JsonObject,
  TranscriptContext,
} from '@earendil-works/pi-ai';
import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';
import workers from '../../extensions/workers/index.ts';
import {hostModule, hostPins} from '../fixtures/host.ts';
import {fixtureModel} from '../fixtures/worker-skills-provider.mjs';

const host: typeof import('@earendil-works/pi-coding-agent') = await import(
  hostModule('pi-coding-agent')
);
const ai: typeof import('@earendil-works/pi-ai') = await import(
  hostModule('pi-ai')
);
const provider = fileURLToPath(
  new URL('../fixtures/worker-skills-provider.mjs', import.meta.url),
);

type Message = TranscriptContext['messages'][number];
type Call = {name: string; arguments: JsonObject};
type Report = {
  skills: {name: string; location: string; content: string}[];
  bodies: string[];
  cwd: string;
  toolCalls: string[];
  priorTurns: number;
};

const text = (message: Message | undefined) =>
  JSON.stringify(message?.content ?? '');
/** Worker events also reach the model as user messages; only our prompts start a script. */
const prompted = (message: Message | undefined, marker: string) =>
  message?.role === 'user' && text(message).includes(marker);

function skill(dir: string, name: string, extra = ''): string {
  mkdirSync(dir, {recursive: true});
  const file = join(dir, 'SKILL.md');
  writeFileSync(
    file,
    `---\nname: ${name}\ndescription: Fixture ${name} workflow for handoff tests.\n${extra}---\n\nBODY-${name}: follow the ${name} steps.\n`,
  );
  return file;
}

async function until(predicate: () => boolean, ms = 20_000) {
  const deadline = Date.now() + ms;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error('workers skills host deadline');
    await sleep(20);
  }
}

/** The block content the child should see for a skill file as it is now. */
const loaded = (file: string) =>
  `References are relative to ${dirname(file)}.\n\n${readFileSync(file, 'utf8').trimEnd()}`;

test(
  'selected model-visible skills reach a real child in full and are reread on continuation',
  {timeout: 90_000},
  async () => {
    assert.equal(
      execFileSync('pi', ['--version'], {encoding: 'utf8'}).trim(),
      hostPins['@earendil-works/pi-coding-agent'],
    );
    const dir = realpathSync(
      mkdtempSync(join(tmpdir(), 'dots-worker-skills-')),
    );
    const saved = {...process.env};
    const agent = join(dir, 'agent');
    const repo = join(dir, 'repo');
    const elsewhere = join(dir, 'elsewhere');
    mkdirSync(elsewhere, {recursive: true});
    execFileSync('git', ['init', '--quiet', repo]);
    const paths = {
      alpha: skill(join(agent, 'skills/alpha'), 'alpha'),
      gamma: skill(join(agent, 'skills/gamma'), 'gamma'),
      hidden: skill(
        join(agent, 'skills/hidden'),
        'hidden',
        'disable-model-invocation: true\n',
      ),
      beta: skill(join(repo, '.agents/skills/beta'), 'beta'),
      // Discovered, then removed before selection.
      delta: skill(join(agent, 'skills/delta'), 'delta'),
    };
    const sessions = join(agent, 'dots/workers');
    const workerDirs = () =>
      existsSync(sessions) ? readdirSync(sessions).sort() : [];
    process.env.HOME = dir;
    process.env.PI_CODING_AGENT_DIR = agent;
    process.env.XDG_STATE_HOME = join(dir, 'state');
    process.env.XDG_DATA_HOME = join(dir, 'data');
    process.env.PI_OFFLINE = '1';
    for (const key of [
      'DOTS_WORKER_ID',
      'DOTS_WORKER_ROOT',
      'DOTS_WORKER_DEPTH',
      'REX_SESSION',
      'REX_BLOCK',
    ])
      delete process.env[key];
    // The children's settings; skill commands off shows the catalog ignores that setting.
    const childSettings = {
      extensions: [provider],
      packages: [],
      defaultProvider: fixtureModel.provider,
      defaultModel: fixtureModel.id,
      enableSkillCommands: false,
      compaction: {enabled: false},
      retry: {enabled: false},
      cacheWarming: 'off',
    };
    writeFileSync(join(agent, 'settings.json'), JSON.stringify(childSettings));
    const settings = host.SettingsManager.inMemory({
      enableSkillCommands: false,
      compaction: {enabled: false},
      retry: {enabled: false},
      cacheWarming: 'off',
      defaultThinkingLevel: 'off',
    });
    settings.setProjectTrusted(true);
    const loader = new host.DefaultResourceLoader({
      cwd: repo,
      agentDir: agent,
      settingsManager: settings,
      noExtensions: true,
      noPromptTemplates: true,
      noThemes: true,
      extensionFactories: [
        // An earlier extension hides gamma from this run's prompt.
        (api: ExtensionAPI) =>
          api.on('before_agent_start', event => {
            event.systemPromptOptions.skills =
              event.systemPromptOptions.skills.filter(s => s.name !== 'gamma');
          }),
        (api: ExtensionAPI) => workers(api),
      ],
      // An SDK host may inject skills with no file; they must not block others.
      skillsOverride: base => ({
        ...base,
        skills: [
          ...base.skills,
          {
            name: 'virtual',
            description: 'Fixture virtual workflow for handoff tests.',
            filePath: '<inline:virtual>',
            baseDir: '<inline:virtual>',
            sourceInfo: host.createSyntheticSourceInfo('<inline:virtual>', {
              source: 'inline',
            }),
            disableModelInvocation: false,
          },
        ],
      }),
    });
    await loader.reload();
    assert.deepEqual(
      loader
        .getSkills()
        .skills.map(s => s.name)
        .sort(),
      ['alpha', 'beta', 'delta', 'gamma', 'hidden', 'virtual'],
      'the parent discovers user and project skills',
    );
    assert.ok(
      loader.getSkills().skills.find(s => s.name === 'hidden')
        ?.disableModelInvocation,
    );
    const runtime = await host.ModelRuntime.create({
      authPath: join(dir, 'auth.json'),
      modelsPath: null,
      modelsStorePath: join(dir, 'models-store.json'),
      allowModelNetwork: false,
      refreshOnCreate: false,
    });
    let worker: string | undefined;
    let calls = 0;
    const handled = new Set<string>();
    let script: (last: Message | undefined) => Call | undefined = () =>
      undefined;
    // The parent scripts the same model ref the children's fixture provider serves, so `inherit` resolves in both.
    const {provider: providerName, api, baseUrl, ...parentModel} = fixtureModel;
    runtime.registerProvider(providerName, {
      api,
      apiKey: 'fixture',
      baseUrl,
      models: [{...parentModel, input: ['text']}],
      streamSimple(model, context) {
        const stream = ai.createAssistantMessageEventStream();
        // Worker events can start turns between steps; each step answers once.
        const last = context.messages.findLast(
          message =>
            message.role === 'toolResult' ||
            prompted(message, 'start-skills') ||
            prompted(message, 'continue-skills'),
        );
        const key = last?.role === 'toolResult' ? last.toolCallId : text(last);
        const fresh = !handled.has(key);
        handled.add(key);
        if (
          fresh &&
          last?.role === 'toolResult' &&
          last.toolName === 'worker_spawn' &&
          !last.isError
        )
          worker = /Spawned (w-[0-9a-f]{16})/.exec(text(last))?.[1];
        const call = fresh ? script(last) : undefined;
        const reason = call ? ('toolUse' as const) : ('stop' as const);
        const message: AssistantMessage = {
          role: 'assistant',
          content: call
            ? [
                {
                  type: 'toolCall',
                  id: `call-${++calls}`,
                  name: call.name,
                  arguments: call.arguments,
                },
              ]
            : [{type: 'text', text: 'noted'}],
          api: model.api,
          provider: model.provider,
          model: model.id,
          timestamp: Date.now(),
          stopReason: reason,
          usage: {
            input: 1,
            output: 1,
            cacheRead: 0,
            cacheWrite: 0,
            totalTokens: 2,
            cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0},
          },
        };
        queueMicrotask(() => {
          stream.push({type: 'start', partial: message});
          stream.push({type: 'done', reason, message});
          stream.end();
        });
        return stream;
      },
    });
    const model = runtime.getModel(providerName, fixtureModel.id);
    assert.ok(model);
    const errors: unknown[] = [];
    const {session} = await host.createAgentSession({
      cwd: repo,
      agentDir: agent,
      modelRuntime: runtime,
      model,
      settingsManager: settings,
      sessionManager: host.SessionManager.inMemory(repo),
      resourceLoader: loader,
      noTools: 'builtin',
    });
    await session.bindExtensions({
      mode: 'rpc',
      onError: error => errors.push(error),
    });
    const results = (name: string) =>
      session.messages.flatMap(message =>
        message.role === 'toolResult' && message.toolName === name
          ? [message]
          : [],
      );
    /** Until the script reached `count` results of `tool` and the session is idle. */
    const idle = async (tool?: string, count = 1) => {
      await until(() => !tool || results(tool).length >= count);
      await sleep(300);
      await until(() => !session.isStreaming);
    };
    const details = (message: Message | undefined) =>
      (message as {details?: Record<string, unknown>} | undefined)?.details ??
      {};
    const report = (message: Message | undefined): Report =>
      JSON.parse(String(details(message).result));
    const spawn = (skills?: string[]): Call => ({
      name: 'worker_spawn',
      arguments: {
        label: skills ? 'skilled' : 'plain',
        task: 'Do the assignment',
        model: 'inherit',
        thinking: 'inherit',
        cwd: elsewhere,
        tools: ['ls'],
        ...(skills ? {skills} : {}),
      },
    });
    try {
      let spawns = 0;
      // Each rejected selection retries with the next; the last one succeeds.
      const attempts = [
        ['beta', 'missing'],
        ['hidden'],
        ['gamma'],
        ['virtual'],
        ['beta', 'delta'],
        ['beta', 'alpha', 'beta'],
      ];
      script = last => {
        if (prompted(last, 'start-skills')) {
          // Discovered for this run, unreadable by the time it is selected.
          rmSync(paths.delta);
          return spawn(attempts.shift());
        }
        if (last?.role === 'toolResult' && last.toolName === 'worker_spawn') {
          if (last.isError) return spawn(attempts.shift());
          return {
            name: 'worker_wait',
            arguments: {id: worker ?? '', timeoutMs: 20_000},
          };
        }
        if (last?.role === 'toolResult' && last.toolName === 'worker_wait')
          return ++spawns === 1 ? spawn() : undefined;
        return undefined;
      };
      await session.prompt('start-skills');
      await idle('worker_wait', 2);

      assert.deepEqual(attempts, []);
      const [rejected, hidden, omitted, virtual, unreadable, skilled, plain] =
        results('worker_spawn');
      const choices = /Available skills: alpha, beta, delta\./;
      assert.equal(rejected?.isError, true);
      assert.match(
        text(rejected),
        /Unknown skill: missing\./,
        "valid choices are the run's model-visible skills with files",
      );
      assert.match(text(rejected), choices);
      assert.equal(hidden?.isError, true);
      assert.match(
        text(hidden),
        /Unknown skill: hidden\./,
        'a command-only skill cannot be selected',
      );
      assert.match(text(hidden), choices);
      assert.ok(!text(hidden).includes(paths.hidden));
      assert.equal(omitted?.isError, true);
      assert.match(
        text(omitted),
        /Unknown skill: gamma\./,
        'a skill an earlier extension omitted cannot be selected',
      );
      assert.match(text(omitted), choices);
      assert.equal(virtual?.isError, true);
      assert.match(
        text(virtual),
        /"Pi reports no skill file to hand off for virtual \(\\"<inline:virtual>\\"\)\."/,
        'a virtual skill gets an explicit diagnostic, not a miss',
      );
      assert.equal(unreadable?.isError, true);
      assert.ok(
        text(unreadable).includes(
          `Cannot read selected skill file: delta (${paths.delta}): ENOENT`,
        ),
        text(unreadable),
      );
      assert.ok(!text(unreadable).includes('beta ('), 'only the failure');
      assert.match(text(skilled), /skills beta, alpha/);
      const selected = [
        {name: 'beta', path: paths.beta},
        {name: 'alpha', path: paths.alpha},
      ];
      assert.deepEqual(details(skilled).skills, selected);
      assert.ok(plain, 'the plain spawn ran');
      assert.equal(details(plain).skills, undefined);

      const [skilledWait, plainWait] = results('worker_wait');
      assert.equal(details(skilledWait).state, 'completed', text(skilledWait));
      const first = report(skilledWait);
      assert.equal(first.cwd, elsewhere, 'the child runs in its own cwd');
      assert.equal(first.priorTurns, 0, 'reported from the first request');
      assert.deepEqual(first.toolCalls, [], 'the child read nothing');
      assert.deepEqual(
        first.skills,
        selected.map(({name, path}) => ({
          name,
          location: path,
          content: loaded(path),
        })),
        'each selected file in full, in order, once, from outside its cwd',
      );
      assert.deepEqual(
        first.bodies,
        ['BODY-beta', 'BODY-alpha'],
        'unselected, hidden, and omitted bodies stay out',
      );
      assert.equal(details(plainWait).state, 'completed', text(plainWait));
      const second = report(plainWait);
      assert.deepEqual(second.skills, [], 'omission hands off nothing');
      assert.deepEqual(second.bodies, []);
      assert.deepEqual(second.toolCalls, []);

      const id = details(skilled).id;
      assert.equal(typeof id, 'string');
      const recordsOf = (worker: unknown) =>
        session.sessionManager
          .getEntries()
          .flatMap(entry =>
            entry.type === 'custom' &&
            entry.customType === 'dots.worker' &&
            (entry.data as {id?: string}).id === worker
              ? [entry.data as {skills?: unknown}]
              : [],
          );
      const records = recordsOf(id);
      assert.ok(records.length);
      for (const record of records) assert.deepEqual(record.skills, selected);
      assert.deepEqual(
        workerDirs(),
        [String(id), String(details(plain).id)].sort(),
        'the unreadable selection allocated no session',
      );
      assert.equal(
        new Set(
          session.sessionManager
            .getEntries()
            .flatMap(entry =>
              entry.type === 'custom' && entry.customType === 'dots.worker'
                ? [(entry.data as {id?: string}).id]
                : [],
            ),
        ).size,
        2,
        'the unreadable selection recorded no worker',
      );

      // Reload rebuilds the worker from its record; continuation relaunches the
      // child on its session and reads the selected files again: an unreadable
      // one fails the delivery without a process, and an edit reaches the child.
      await session.reload();
      await idle();
      const revised = `${readFileSync(paths.beta, 'utf8')}\nREVISED: beta changed after spawn.\n`;
      let beforeMissing: unknown;
      script = last => {
        // The replacement runtime learns visibility from its own first run.
        if (prompted(last, 'continue-skills')) return spawn(['hidden']);
        const again = {
          name: 'message',
          arguments: {
            to: String(id),
            text: 'Again',
            wait: true,
            timeoutMs: 20_000,
          },
        };
        if (
          last?.role === 'toolResult' &&
          last.toolName === 'worker_spawn' &&
          last.isError
        ) {
          beforeMissing = recordsOf(id);
          rmSync(paths.beta);
          return again;
        }
        if (last?.role === 'toolResult' && last.toolName === 'message') {
          if (!last.isError)
            return {name: 'worker_status', arguments: {id: String(id)}};
          assert.deepEqual(
            recordsOf(id),
            beforeMissing,
            'failed continuation leaves the recorded state unchanged',
          );
          writeFileSync(paths.beta, revised);
          return again;
        }
        return undefined;
      };
      await session.prompt('continue-skills');
      await idle('worker_status');
      assert.match(
        text(results('worker_spawn').at(-1)),
        /Unknown skill: hidden\. Available skills: alpha, beta\./,
        'reload keeps command-only skills unselectable',
      );
      assert.equal(results('worker_spawn').length, 8);
      const [missing, continued] = results('message');
      assert.equal(missing?.isError, true);
      assert.ok(
        text(missing).includes(
          `Cannot read selected skill file: beta (${paths.beta}): ENOENT`,
        ),
        text(missing),
      );
      assert.deepEqual(
        workerDirs(),
        [String(id), String(details(plain).id)].sort(),
      );
      assert.equal(details(continued).state, 'completed', text(continued));
      assert.deepEqual(details(continued).skills, selected);
      const resumed = report(continued);
      assert.deepEqual(resumed.toolCalls, []);
      assert.ok(resumed.priorTurns > 0, 'the child continued its session');
      assert.deepEqual(
        resumed.skills,
        selected.map(({name, path}) => ({
          name,
          location: path,
          content: loaded(path),
        })),
        'the restart reads the current files',
      );
      assert.match(resumed.skills[0]?.content ?? '', /REVISED: beta changed/);
      for (const record of recordsOf(id))
        assert.deepEqual(record.skills, selected, 'records keep references');
      const status = results('worker_status').at(-1);
      assert.deepEqual(details(status).skills, selected);
      assert.ok(
        text(status).includes(`skill: beta ${paths.beta}`),
        text(status),
      );
    } finally {
      await session.abort();
      await session.extensionRunner.emit({
        type: 'session_shutdown',
        reason: 'quit',
      });
      session.dispose();
      process.env = saved;
      rmSync(dir, {recursive: true, force: true});
    }
    assert.deepEqual(errors, []);
  },
);
