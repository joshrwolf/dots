// Cross-extension prompt and recovery contract against the native host, provider-free.
import assert from 'node:assert/strict';
import {mkdtempSync, existsSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import test from 'node:test';
import goal, {FOCUS_ENTRY} from '../../extensions/goal/index.ts';
import {snapshot, source} from '../goal-fixture.mjs';
import memory from '../../extensions/memory/index.ts';
import rex from '../../extensions/rex/index.ts';
import {goalSectionKey} from '../../extensions/goal/context.ts';
import {contextReport} from '../../extensions/context/index.ts';
import {hostModule} from '../fixtures/host.ts';
const host = await import(hostModule('pi-coding-agent'));
const ai = await import(hostModule('pi-ai'));

function promptSections(context) {
  const sections = {};
  for (const message of context.messages) {
    if (message.role !== 'system') continue;
    for (const [name, value] of Object.entries(message.sections ?? {})) {
      if (value === null) delete sections[name];
      else sections[name] = value;
    }
  }
  return Object.values(sections).join('\n');
}

test(
  'native sections isolate updates and survive same-run focus, events, compaction, resume and tree navigation',
  {timeout: 20_000},
  async () => {
    const dir = mkdtempSync(join(tmpdir(), 'dots-goal-host-'));
    const previous = process.env.PI_CODING_AGENT_DIR;
    process.env.PI_CODING_AGENT_DIR = dir;
    // The session is its own coordinator, not a worker of the launching agent.
    const inherited = [
      'DOTS_WORKER_ID',
      'DOTS_AGENT_OWNER',
      'PI_SESSION_ID',
    ].map(key => [key, process.env[key]]);
    for (const [key] of inherited) delete process.env[key];
    // Goals and coordination come from fixture records: no catalog, nothing
    // from this machine.
    const backend = source();
    const requests = [];
    const errors = [];
    const reports = [];
    const memorySections = {
      dots_memory_global: 'Global fixture knowledge',
      dots_memory_project: 'Project fixture knowledge',
    };
    let clock = 100;
    let taskTitle = 'Reference slice';
    let unavailable = false;
    let rootOwner = null;
    let promptOptions;
    backend.show = async (_cwd, id) => {
      if (unavailable) throw new Error('Fixture source unavailable');
      const value = snapshot(id);
      value.now = clock;
      value.overview.goal.root.owner = rootOwner;
      value.overview.tasks[0].title = taskTitle;
      if (backend.briefContent)
        value.overview.goal.briefContent = backend.briefContent;
      value.overview.tasks[0].last_event = {
        seq: 7,
        at: 90,
        kind: 'hold',
        actor: 'fixture-agent',
        detail: 'Implemented on a fixture branch; checks pending',
        task: 't-456def',
      };
      return value;
    };
    let focusNext = false;
    let unfocusNext = false;
    const runtime = await host.ModelRuntime.create({
      authPath: join(dir, 'auth.json'),
      modelsPath: null,
      modelsStorePath: join(dir, 'models.json'),
      allowModelNetwork: false,
      refreshOnCreate: false,
    });
    runtime.registerProvider('goal-fixture', {
      api: 'goal-fixture',
      apiKey: 'fixture',
      baseUrl: 'https://example.invalid',
      models: [
        {
          id: 'fixture',
          name: 'Fixture',
          reasoning: false,
          input: ['text'],
          contextWindow: 100_000,
          maxTokens: 1000,
          cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0},
        },
      ],
      streamSimple(model, context) {
        requests.push(structuredClone(context));
        const stream = ai.createAssistantMessageEventStream();
        const message = {
          role: 'assistant',
          content: [{type: 'text', text: 'Fixture response'}],
          api: model.api,
          provider: model.provider,
          model: model.id,
          timestamp: Date.now(),
          stopReason: 'stop',
          usage: {
            input: 1,
            output: 1,
            cacheRead: 0,
            cacheWrite: 0,
            totalTokens: 2,
            cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0},
          },
        };
        if (focusNext || unfocusNext) {
          message.content = [
            {
              type: 'toolCall',
              id: 'focus-fixture',
              name: unfocusNext ? 'goal_unfocus' : 'goal_focus',
              arguments: {id: 't-123abc'},
            },
          ];
          focusNext = false;
          unfocusNext = false;
          message.stopReason = 'toolUse';
        }
        queueMicrotask(() => {
          stream.push({type: 'start', partial: message});
          stream.push({type: 'done', reason: message.stopReason, message});
          stream.end();
        });
        return stream;
      },
    });
    async function create(manager) {
      const settings = host.SettingsManager.inMemory({
        compaction: {enabled: false, keepRecentTokens: 128},
        retry: {enabled: false},
        cacheWarming: 'off',
      });
      const loader = new host.DefaultResourceLoader({
        cwd: dir,
        agentDir: dir,
        settingsManager: settings,
        noExtensions: true,
        noSkills: true,
        noPromptTemplates: true,
        noThemes: true,
        agentsFilesOverride: () => ({
          agentsFiles: [
            {
              path: '/example/AGENTS.md',
              content: 'Native fixture instructions',
            },
          ],
        }),
        extensionFactories: [
          pi => {
            pi.on('before_agent_start', event => {
              promptOptions = event.systemPromptOptions;
            });
            pi.registerCommand('fixture-context-report', {
              description:
                'Inspect context with the actual native command context',
              handler: (_args, ctx) => {
                reports.push(contextReport(pi, ctx));
              },
            });
            memory(pi, async () => ({...memorySections}));
            rex(
              pi,
              {REX_SESSION: 'fixture'},
              async () => 'Neutral terminal orientation',
            );
            pi.on('session_before_compact', event => ({
              compaction: {
                summary:
                  'Fixture checkpoint; consult the current goal agreement.',
                firstKeptEntryId: event.preparation.firstKeptEntryId,
                tokensBefore: event.preparation.tokensBefore,
              },
            }));
          },
          pi =>
            goal(
              pi,
              backend,
              {
                async record(_cwd, id, signal, seq, project) {
                  assert.equal(signal.aborted, false);
                  return {
                    task: id,
                    project,
                    seq,
                    current: 10,
                    result: 'Recorded fixture outcome',
                    evidence: [
                      {slug: 'report', digest: 'sha256:' + 'a'.repeat(64)},
                    ],
                    purged: [],
                  };
                },
                async page() {
                  throw new Error(
                    'Headless inspection must not read report bytes',
                  );
                },
              },
              {changes: async () => undefined},
            ),
        ],
      });
      await loader.reload();
      const {session} = await host.createAgentSession({
        cwd: dir,
        agentDir: dir,
        settingsManager: settings,
        resourceLoader: loader,
        modelRuntime: runtime,
        model: runtime.getModel('goal-fixture', 'fixture'),
        sessionManager: manager,
        noTools: 'builtin',
      });
      await session.bindExtensions({
        mode: 'rpc',
        onError: error => errors.push(error),
      });
      return session;
    }
    async function close(session) {
      await session.abort();
      await session.extensionRunner.emit({
        type: 'session_shutdown',
        reason: 'quit',
      });
      session.dispose();
    }
    let session;
    try {
      session = await create(
        host.SessionManager.create(dir, join(dir, 'sessions')),
      );
      await session.prompt('/goal evidence t-456def 9 p-fixture');
      assert.equal(requests.length, 0, 'evidence inspection must not prompt');
      assert.match(
        JSON.stringify(session.sessionManager.getBranch()),
        /dots.goal.evidence/,
      );
      assert.match(
        JSON.stringify(session.sessionManager.getBranch()),
        /Recorded fixture outcome/,
      );
      assert.equal(
        session.sessionManager
          .getBranch()
          .filter(e => e.type === 'custom' && e.customType === FOCUS_ENTRY)
          .length,
        0,
        'task evidence needs no followed goal',
      );
      await session.prompt('/goal focus t-123abc');
      assert.equal(requests.length, 0);
      const saved = session.sessionManager
        .getBranch()
        .find(e => e.type === 'custom' && e.customType === FOCUS_ENTRY);
      assert.equal(saved.data.focus.id, 't-123abc');
      await session.prompt('Discuss the design; do not execute work.');
      assert.equal(requests.length, 1);
      assert.match(
        promptSections(requests.at(-1)),
        /Use one shared export interface/,
      );
      assert.match(
        promptSections(requests.at(-1)),
        /t-456def \[open\] Reference slice/,
      );
      assert.match(
        promptSections(requests.at(-1)),
        /Results and history: dots task/,
      );
      await session.prompt('/fixture-context-report');
      const currentReport = reports.at(-1);
      assert.match(currentReport, /System prompt \(recorded effective\)/);
      assert.equal(
        'dots_goal_coordinating' in promptOptions.sections,
        false,
        'coordination was read, not unavailable',
      );
      // Five sections fit the report without an overflow count.
      assert.match(currentReport, /Companion sections \(5\)/);
      assert.doesNotMatch(currentReport, /… \d+ more/);
      for (const section of [
        'dots_memory_global',
        'dots_memory_project',
        'dots_rex',
      ])
        assert.ok(section in promptOptions.sections, section);
      assert.match(
        currentReport,
        new RegExp(
          `${goalSectionKey({
            focus: {id: 't-123abc', project: '/example/project'},
          })}_tasks`,
        ),
      );
      assert.equal(
        requests.length,
        1,
        'context inspection must not call a model',
      );
      const anchor = session.sessionManager.getLeafId();
      const file = session.sessionFile;
      assert.ok(file && existsSync(file));
      await close(session);
      session = undefined;
      backend.briefContent = {
        text: 'Fresh accepted goal pattern after restart',
      };
      session = await create(host.SessionManager.open(file));
      assert.equal(requests.length, 1, 'restore must not prompt');
      await session.prompt('/goal show');
      assert.equal(requests.length, 1, 'inspection must not prompt');
      await session.prompt('Discuss the revised brief.');
      assert.equal(requests.length, 2);
      assert.match(
        promptSections(requests.at(-1)),
        /Fresh accepted goal pattern after restart/,
      );
      assert.doesNotMatch(
        promptSections(requests.at(-1)),
        /Use one shared export interface/,
      );
      await session.prompt('/goal unfocus');
      await session.prompt('Unrelated discussion.');
      assert.equal(requests.length, 3);
      assert.doesNotMatch(
        promptSections(requests.at(-1)),
        /Focused goal context|Fresh accepted goal pattern after restart/,
      );
      focusNext = true;
      const planned = new Promise(resolve => {
        const unsubscribe = session.subscribe(event => {
          if (event.type === 'agent_settled') {
            unsubscribe();
            resolve();
          }
        });
      });
      await session.prompt('/goal new Consistent fixture exports');
      await planned;
      assert.equal(
        requests.length,
        5,
        'new starts planning; tool use gets one normal follow-up',
      );
      assert.match(
        JSON.stringify(requests.at(-2)),
        /Consistent fixture exports/,
      );
      const latest = session.sessionManager
        .getBranch()
        .filter(e => e.type === 'custom' && e.customType === FOCUS_ENTRY)
        .at(-1);
      assert.equal(latest.data.focus.id, 't-123abc');
      assert.ok(
        session.messages.some(
          m =>
            m.role === 'toolResult' &&
            m.toolName === 'goal_focus' &&
            !m.isError,
        ),
      );
      const focusedResult = session.messages.find(
        message =>
          message.role === 'toolResult' && message.toolName === 'goal_focus',
      );
      assert.match(
        focusedResult.content.map(part => part.text ?? '').join('\n'),
        /Fresh accepted goal pattern after restart/,
      );
      assert.doesNotMatch(
        focusedResult.content.map(part => part.text ?? '').join('\n'),
        /Recorded at:|Contributing workers/,
      );
      await session.prompt('/goal follow t-999999');
      const pairFile = session.sessionFile;
      await close(session);
      session = await create(host.SessionManager.open(pairFile));
      await session.prompt('Discuss both followed goals.');
      assert.match(promptSections(requests.at(-1)), /Goal t-123abc/);
      assert.match(promptSections(requests.at(-1)), /Goal t-999999/);
      unfocusNext = true;
      await session.prompt(
        'Stop following the first goal, leaving the second alone.',
      );
      assert.ok(
        session.messages.some(
          m =>
            m.role === 'toolResult' &&
            m.toolName === 'goal_unfocus' &&
            !m.isError,
        ),
      );
      await session.prompt('Discuss the remaining goal.');
      assert.doesNotMatch(promptSections(requests.at(-1)), /Goal t-123abc/);
      assert.match(promptSections(requests.at(-1)), /Goal t-999999/);
      // Cross-extension long-horizon contract: idle observation and background
      // notifications do not replay snapshots or mutate native instructions.
      const key = goalSectionKey({
        focus: {id: 't-999999', project: '/example/project'},
      });
      const patchEntries = () =>
        session.sessionManager
          .buildSessionProjection()
          .messages.filter(message => message.role === 'system');
      const stable = patchEntries().length;
      for (const customType of Array.from({length: 10}, () => [
        'dots.worker.event',
        'dots-job-result',
        'dots.wake',
      ]).flat()) {
        clock += 61;
        await session.sendCustomMessage(
          {customType, content: 'Neutral material event', display: false},
          {triggerTurn: true},
        );
      }
      assert.equal(
        patchEntries().length,
        stable,
        'events and clocks cannot replay unchanged snapshots',
      );
      const options = promptOptions;
      assert.deepEqual(options.contextFiles, [
        {path: '/example/AGENTS.md', content: 'Native fixture instructions'},
      ]);
      assert.match(
        options.sections[`${key}_brief`],
        /Fresh accepted goal pattern/,
      );
      assert.match(
        options.sections.dots_memory_global,
        /Global fixture knowledge/,
      );
      assert.equal(options.sections.dots_rex, 'Neutral terminal orientation');
      const expectPatch = expected => {
        const latest = patchEntries().at(-1);
        assert.deepEqual(Object.keys(latest.sections), [expected]);
      };
      rootOwner = 'fixture-owner';
      await session.prompt('Observe root ownership.');
      expectPatch(`${key}_tasks`);
      assert.match(
        promptSections(requests.at(-1)),
        /\[open\] · claimed by fixture-owner/,
      );
      const beforeTick = patchEntries().length;
      clock += 3600;
      await session.prompt('Observe a later clock.');
      assert.equal(
        patchEntries().length,
        beforeTick,
        'time alone does not change semantic state',
      );
      rootOwner = null;
      await session.prompt('Observe root release.');
      expectPatch(`${key}_tasks`);
      assert.doesNotMatch(promptSections(requests.at(-1)), /claimed by/);
      taskTitle = 'Revised fixture slice';
      await session.prompt('Observe revised task state.');
      expectPatch(`${key}_tasks`);
      memorySections.dots_memory_project = 'Updated project knowledge';
      await session.prompt('Observe revised project memory.');
      expectPatch('dots_memory_project');
      memorySections.dots_memory_global = 'Updated global knowledge';
      await session.prompt('Observe revised global memory.');
      expectPatch('dots_memory_global');
      backend.briefContent = {
        text: 'Current working agreement survives compaction',
        revision: 'sha256:' + 'c'.repeat(64),
      };
      await session.prompt('Observe revised agreement.');
      expectPatch(`${key}_brief`);
      unavailable = true;
      await session.prompt('Observe unavailable source.');
      assert.match(promptSections(requests.at(-1)), /Brief unavailable/);
      assert.match(promptSections(requests.at(-1)), /Task state unavailable/);
      assert.doesNotMatch(
        promptSections(requests.at(-1)),
        /Current working agreement survives compaction/,
      );
      unavailable = false;
      await session.prompt('Observe restored source.');
      assert.match(
        promptSections(requests.at(-1)),
        /Current working agreement survives compaction/,
      );
      await session.compact();
      const checkpoint = session.sessionManager
        .getBranch()
        .filter(entry => entry.type === 'compaction')
        .at(-1);
      assert.match(
        checkpoint.systemMessage.sections[`${key}_brief`],
        /Current working agreement/,
      );
      assert.equal(
        checkpoint.systemMessage.sections.dots_rex,
        '<dots_rex>\nNeutral terminal orientation\n</dots_rex>',
      );
      const compactFile = session.sessionFile;
      await close(session);
      session = await create(host.SessionManager.open(compactFile));
      const afterResume = patchEntries().length;
      await session.prompt('Continue discussing the current agreement.');
      assert.equal(
        patchEntries().length,
        afterResume,
        'cold resume uses the native checkpoint, not a process-local injection cache',
      );
      assert.match(
        promptSections(requests.at(-1)),
        /Current working agreement/,
      );
      assert.match(
        promptSections(requests.at(-1)),
        /Updated project knowledge/,
      );
      await session.navigateTree(anchor, {summarize: false});
      await session.prompt('Discuss the original branch.');
      assert.match(promptSections(requests.at(-1)), /Goal t-123abc/);
      assert.doesNotMatch(promptSections(requests.at(-1)), /Goal t-999999/);
      assert.ok(
        backend.coordinations.length > 0,
        'coordination reads went to the fixture',
      );
      assert.deepEqual(errors, []);
    } finally {
      if (session) await close(session);
      if (previous === undefined) delete process.env.PI_CODING_AGENT_DIR;
      else process.env.PI_CODING_AGENT_DIR = previous;
      for (const [key, value] of inherited)
        if (value === undefined) delete process.env[key];
        else process.env[key] = value;
      rmSync(dir, {recursive: true, force: true});
    }
  },
);
