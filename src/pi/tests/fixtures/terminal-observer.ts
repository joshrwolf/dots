import {appendFileSync} from 'node:fs';
import assert from 'node:assert/strict';
import type {ExtensionAPI} from '@earendil-works/pi-coding-agent';
import {JobsPresentation} from '../../extensions/jobs/ui.ts';
import {WorkerInspection} from '../../extensions/workers/inspection.ts';
import type {WorkerInspectionSource} from '../../extensions/workers/inspector.ts';
import {jobFixture} from '../../dev/scenarios.ts';

export default function (pi: ExtensionAPI): void {
  const file = process.env['DOTS_TUI_MARKER'];
  if (!file) throw new Error('Missing terminal-test marker path');
  const mark = (text: string): void => {
    appendFileSync(file, text + '\n');
  };
  const subscriptions: (() => void)[] = [];
  const inspections = new WorkerInspection();
  let jobs: JobsPresentation | undefined;
  let closed = false;
  pi.on('session_start', (_event, ctx) => {
    mark('start');
    if (process.env['DOTS_UI_WORKBENCH'] !== '1') return;
    subscriptions.push(
      pi.events.on('dots:workbench', state =>
        mark('workbench ' + JSON.stringify(state)),
      ),
      pi.events.on('dots:workbench-closed', () => mark('workbench closed')),
      pi.events.on('dots:workbench-input', () =>
        mark('workbench input handled'),
      ),
      pi.events.on('dots:workbench-confirmed', approved =>
        mark('confirmation ' + String(approved)),
      ),
    );
    let previous = '';
    ctx.ui.setWidget('accept-render-probe', () => ({
      invalidate() {},
      render(width) {
        const next = `frame ${width} ${ctx.ui.theme.appearance}`;
        if (previous !== next) {
          previous = next;
          mark(next);
        }
        return [];
      },
    }));
  });
  pi.on('session_shutdown', () => {
    closed = true;
    jobs?.dispose(false);
    inspections.dispose();
    for (const unsubscribe of subscriptions.splice(0)) unsubscribe();
    mark('shutdown');
  });
  pi.on('agent_start', () => mark('UNEXPECTED_PROVIDER_TURN'));
  pi.registerCommand('accept-job-inspect', {
    async handler(_args, ctx) {
      const job = jobFixture(5); // Historical fixture: no process, timer or stop control.
      jobs?.dispose();
      jobs = new JobsPresentation(ctx, {
        list: () => [job],
        get: () => job,
        hasLive: () => false,
        stop: () => {
          throw new Error('Fixture must not stop');
        },
      });
      jobs.refresh();
      ctx.ui.setEditorText('keep this activity draft');
      await jobs.open(job.id);
      if (!closed) {
        mark(
          'job inspection draft ' +
            (ctx.ui.getEditorText() === 'keep this activity draft'),
        );
        ctx.ui.setEditorText('');
        jobs.dispose();
      }
    },
  });
  pi.registerCommand('accept-worker-inspect', {
    async handler(_args, ctx) {
      let state = 'running';
      let text =
        'Question: Which revision?\n' +
        Array.from({length: 60}, (_, i) => `Recorded line ${i}`).join('\n');
      const owner = {
        id: 'w-parent',
        label: 'Review',
        state,
        depth: 0,
        canMessage: true,
        canStop: true,
      };
      const child = {
        id: 'w-child',
        label: 'Child',
        state: 'running',
        depth: 1,
        owner: 'w-parent',
        canMessage: false,
        canStop: false,
      };
      const source: WorkerInspectionSource = {
        list: () => [owner, child],
        read: id =>
          id === owner.id
            ? {
                worker: {...owner, state},
                detail: `Directory: /fixture/review\n${state}`,
                messages: [
                  {role: 'assistant', content: [{type: 'text', text}]},
                ],
              }
            : id === child.id
              ? {
                  worker: child,
                  detail: 'Directory: /fixture/child',
                  messages: [
                    {
                      role: 'assistant',
                      content: [{type: 'text', text: 'Child response'}],
                    },
                  ],
                }
              : undefined,
        message: async (id, value) => {
          assert(id === owner.id, 'descendant must not be messaged');
          if (value === 'reject') {
            mark('worker-message-rejected');
            throw new Error('fixture delivery failed');
          }
          mark(`worker-message:${value}`);
        },
        stop: async id => {
          assert(id === owner.id, 'descendant must not be stopped');
          state = 'stopped';
          owner.state = state;
          owner.canStop = false;
          mark('worker-stopped');
        },
      };
      ctx.ui.setEditorText('keep this activity draft');
      const update = setTimeout(() => {
        text += '\nSTREAMED_ACCEPTANCE_TEXT';
        inspections.record(owner.id, {
          type: 'message_update',
          assistantMessageEvent: {
            type: 'text_delta',
            contentIndex: 0,
            delta: 'STREAMED_ACCEPTANCE_TEXT',
          },
        });
        mark('worker-streamed');
      }, 900);
      const settled = setTimeout(() => {
        state = 'completed';
        owner.state = state;
        inspections.record(owner.id, {type: 'message_end'});
        mark('worker-completed');
      }, 1500);
      try {
        await inspections.open(ctx, source);
      } finally {
        clearTimeout(update);
        clearTimeout(settled);
      }
      if (!closed) {
        mark(
          'worker inspection draft ' +
            (ctx.ui.getEditorText() === 'keep this activity draft'),
        );
        ctx.ui.setEditorText('');
      }
    },
  });
  pi.registerCommand('accept-theme', {
    async handler(_args, ctx) {
      mark('theme ' + ctx.ui.setTheme('light').success);
    },
  });
  pi.registerCommand('accept-exit', {
    async handler(_args, ctx) {
      ctx.shutdown();
    },
  });
}
