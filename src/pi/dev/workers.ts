import {mkdtempSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import type {Scenario} from './scenarios.ts';
import {SessionFixture} from './sessions.ts';
import {
  WorkerInspector,
  type WorkerSnapshot,
} from '../extensions/workers/inspector.ts';

/** Interactive shipping inspector over neutral session files and in-memory actions. */
export const workerInspectorScenario: Scenario = {
  id: 'worker-inspector',
  title: 'Worker inspection',
  phases: ['interactive fixtures'],
  create(context, theme) {
    const directory = mkdtempSync(path.join(tmpdir(), 'dots-worker-view-'));
    const sessions = new Map<string, SessionFixture>();
    const session = (id: string): SessionFixture => {
      let fixture = sessions.get(id);
      if (!fixture) {
        fixture = new SessionFixture(
          path.join(directory, `${id}.jsonl`),
          `/workspace/${id}`,
        );
        sessions.set(id, fixture);
      }
      return fixture;
    };
    const review = session('w-review');
    review.user('Review the public API and its validation.');
    for (let i = 0; i < 8; i++) {
      const call = `call-${i}`;
      review.assistant(
        [
          {
            type: 'text',
            text: `### Review observation ${i + 1}\n\nChecking validation and error boundaries in module ${i + 1}.\n\n- Inputs stay bounded.\n- Errors retain useful context.`,
          },
          {
            type: 'toolCall',
            id: call,
            name: 'read',
            arguments: {path: `src/module-${i + 1}.ts`},
          },
        ],
        'toolUse',
      );
      review.toolResult(
        call,
        'read',
        `export function module${i + 1}(input: string): string {\n  return input.trim();\n}`,
      );
    }
    session('w-schema').assistant([
      {
        type: 'text',
        text: 'Should unknown fields be rejected? Waiting for my owner.',
      },
    ]);
    session('w-docs').assistant([
      {
        type: 'text',
        text: 'Documentation checked. Examples match the public API.',
      },
    ]);
    const records: WorkerSnapshot[] = [
      {
        worker: {
          id: 'w-review',
          label: 'api-review',
          state: 'running',
          depth: 0,
          canMessage: true,
          canStop: true,
        },
        detail: 'Directory /workspace/api-review · ⎇ feature/api · context 38%',
        sessionFile: review.file,
      },
      {
        worker: {
          id: 'w-schema',
          label: 'schema-check',
          state: 'asking',
          depth: 1,
          owner: 'api-review',
          canMessage: false,
          canStop: false,
        },
        detail:
          'Directory /workspace/schema-check · ⎇ fix/schema · context 82%',
        sessionFile: session('w-schema').file,
      },
      {
        worker: {
          id: 'w-docs',
          label: 'docs',
          state: 'completed',
          depth: 0,
          canMessage: true,
          canStop: false,
        },
        detail: 'Directory /workspace/example · context 12%',
        sessionFile: session('w-docs').file,
      },
      ...Array.from({length: 18}, (_, index): WorkerSnapshot => ({
        worker: {
          id: `w-review-${index + 1}`,
          label: `review-${index + 1}`,
          state: 'running',
          depth: 0,
          canMessage: true,
          canStop: true,
        },
        detail: `Directory /workspace/review-${index + 1} · context 38%`,
      })),
    ];
    const inspector = new WorkerInspector(
      {
        list: () => records.map(record => record.worker),
        read: id => records.find(record => record.worker.id === id),
        async message(id, text) {
          const fixture = session(id);
          fixture.user(text);
          fixture.assistant([
            {
              type: 'text',
              text: 'Fixture message received. No worker process was contacted.',
            },
          ]);
        },
        async stop(id) {
          const record = records.find(item => item.worker.id === id)!;
          record.worker.state = 'stopped';
          record.worker.canStop = false;
          session(id).assistant([
            {
              type: 'text',
              text: 'Fixture stopped; partial review remains available.',
            },
          ]);
        },
      },
      {
        tui: context.tui,
        theme,
        bindings: context.bindings,
        close: context.close,
        height: context.height ?? (() => 24),
        transcript: {
          outputPad: 1,
          hideThinkingBlock: false,
          showImages: false,
          imageWidthCells: 60,
        },
      },
    );
    inspector.focused = true;
    return {
      component: inspector,
      interactive: true,
      update() {
        inspector.refresh();
      },
      dispose() {
        inspector.dispose();
        rmSync(directory, {recursive: true, force: true});
      },
    };
  },
};
