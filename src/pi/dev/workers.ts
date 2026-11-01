import type {Scenario} from './scenarios.ts';
import {
  WorkerInspector,
  type WorkerSnapshot,
} from '../extensions/workers/inspector.ts';

/** Interactive shipping inspector with neutral, in-memory records and actions. */
export const workerInspectorScenario: Scenario = {
  id: 'worker-inspector',
  title: 'Worker inspection',
  phases: ['interactive fixtures'],
  create(context, theme) {
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
        messages: [
          {role: 'user', content: 'Review the public API and its validation.'},
          ...Array.from({length: 24}, (_, i) => ({
            role: 'assistant',
            content: `### Review observation ${i + 1}\n\nChecking validation and error boundaries in module ${i + 1}.\n\n- Inputs stay bounded.\n- Errors retain useful context.`,
          })),
        ],
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
        messages: [
          {
            role: 'assistant',
            content: 'Should unknown fields be rejected? Waiting for my owner.',
          },
        ],
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
        messages: [
          {
            role: 'assistant',
            content: 'Documentation checked. Examples match the public API.',
          },
        ],
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
        messages: [],
      })),
    ];
    const inspector = new WorkerInspector(
      {
        list: () => records.map(record => record.worker),
        read: id => records.find(record => record.worker.id === id),
        async message(id, text) {
          const record = records.find(item => item.worker.id === id)!;
          record.messages.push(
            {role: 'user', content: text},
            {
              role: 'assistant',
              content:
                'Fixture message received. No worker process was contacted.',
            },
          );
        },
        async stop(id) {
          const record = records.find(item => item.worker.id === id)!;
          record.worker.state = 'stopped';
          record.worker.canStop = false;
          record.messages.push({
            role: 'assistant',
            content: 'Fixture stopped; partial review remains available.',
          });
        },
      },
      theme,
      context.bindings,
      context.close,
      context.redraw,
      context.height ?? (() => 24),
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
      },
    };
  },
};
