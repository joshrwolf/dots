// Opt-in neutral fixture: real goal extension/components, no CLI or model calls.
import assert from 'node:assert/strict';
import {appendFileSync} from 'node:fs';
import {backgroundCells} from '../../../pi-kit/tests/fixtures/ansi.ts';
import type {
  ExtensionAPI,
  ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import goal from '../../extensions/goal/index.ts';
import {
  withHierarchy,
  type GoalSnapshot,
} from '../../extensions/goal/source.ts';
import {goalFixtures} from '../../dev/goals.ts';
import {attentionStore, request} from '../attention-fixture.mjs';
import {stripTerminalSequences} from '@earendil-works/pi-tui';
import type {AttentionSource} from '../../extensions/goal/attention/source.ts';

export default function (pi: ExtensionAPI): void {
  const mark = (text: string) =>
    appendFileSync(process.env.DOTS_GOAL_MARKER!, `${text}\n`);
  const root = {
    id: 't-123abc',
    title: 'Unified fixture exports',
    status: 'open' as const,
    parent: null,
    children: 1,
    owner: null,
    result: null,
    result_event: null,
    evidence: [],
    purged_evidence: [],
    references: [],
    branch: null,
    blocked_by: [],
    goal: null,
    holds: [],
    last_event: null,
  };
  const snapshot: GoalSnapshot = {
    project: '/example/project',
    now: 100,
    overview: {
      goal: {
        root,
        brief: '/example/brief.md',
        briefContent: {
          text:
            '# Fixture brief\n' +
            Array.from(
              {length: 100},
              (_, i) => `Pattern line ${i}: shared ownership, café 👩‍💻`,
            ).join('\n') +
            '\nEND_OF_GOAL_BRIEF',
        },
      },
      tasks: [
        {
          ...root,
          id: 't-456def',
          title: 'Reference fixture slice',
          parent: root.id,
          children: 0,
          last_event: {
            seq: 7,
            at: 90,
            kind: 'note',
            actor: 'fixture-agent',
            detail: 'Implemented on a fixture branch; café checks pending 👩‍💻',
            task: 't-456def',
          },
        },
      ],
      hidden_closed: 0,
      graph: {
        rows: [{task: 't-456def', lines: ['○']}],
        notes: [],
      },
      open_graph: null,
      progress: {
        total: 1,
        open: 1,
        held: 0,
        done: 0,
        dropped: 0,
        ready: 1,
        claimed: 0,
        review: 0,
      },
    },
  };
  let lastFrame = '';
  type Handler = (data: unknown, ctx: ExtensionContext) => unknown;
  // Every context the extension receives records its overlay's renders and close.
  const observe = <C extends ExtensionContext>(ctx: C): C => {
    const ui: typeof ctx.ui = {
      ...ctx.ui,
      async custom(factory, options) {
        try {
          return await ctx.ui.custom(async (...parameters) => {
            const view = await factory(...parameters);
            const render = view.render.bind(view);
            view.render = width => {
              const lines = render(width);
              mark(`inspector-render:${width}:${lines.length}`);
              const theme = parameters[1];
              const expected = backgroundCells(
                theme.getBgAnsi('customMessageBg') + ' ',
              )[0]!;
              for (const [index, line] of lines.entries()) {
                const cells = backgroundCells(line);
                assert.deepEqual(
                  cells,
                  Array<string>(cells.length).fill(expected),
                  `${theme.name}: popup background at row ${index}`,
                );
              }
              mark(`inspector-surface:${theme.name}`);
              // What one frame shows: the view, the question, the answer field.
              const text = stripTerminalSequences(lines.join('\n'));
              const frame = [
                /› \[ ← Attention/.test(text)
                  ? 'attention'
                  : /› \[ Work/.test(text)
                    ? 'work'
                    : 'panel',
                /Should the first slice/.test(text) ? 'question' : '-',
                /This project only/.test(text) ? 'options' : '-',
                /Answer: (\S*)/.exec(text)?.[1] ?? '-',
                /Answer saved: (.+?) +│/.exec(text)?.[1] ?? '-',
              ].join(':');
              if (frame !== lastFrame) mark(`frame:${(lastFrame = frame)}`);
              return lines;
            };
            return view;
          }, options);
        } finally {
          mark(`editor:${ctx.ui.getEditorText()}`);
          mark('inspector-closed');
        }
      },
    };
    return {...ctx, ui};
  };
  const observed: ExtensionAPI = {
    ...pi,
    on: ((event: string, handler: Handler) =>
      (pi.on as (event: string, handler: Handler) => void)(event, (data, ctx) =>
        handler(data, observe(ctx)),
      )) as ExtensionAPI['on'],
    registerShortcut(key, shortcut) {
      pi.registerShortcut(key, {
        ...shortcut,
        handler: ctx => shortcut.handler(observe(ctx)),
      });
    },
    registerCommand(name, definition) {
      pi.registerCommand(name, {
        ...definition,
        async handler(args, ctx) {
          try {
            await definition.handler(args, observe(ctx));
          } finally {
            mark(`command-end:${args}`);
          }
        },
      });
    },
  };
  goal(
    observed,
    {
      async list() {
        return {project: snapshot.project, goals: [snapshot.overview.goal]};
      },
      async release() {},
      async show(_cwd, id, _signal, history = false) {
        mark('read');
        if (id === 't-abcdef') {
          const nested = goalFixtures(8)[0]!.snapshot!;
          const allTasks = nested.allTasks!.map(task => ({
            ...task,
            parent:
              task.parent === nested.overview.goal.root.id ? id : task.parent,
          }));
          return withHierarchy(
            {
              ...nested,
              project: snapshot.project,
              allTasks,
              overview: {
                ...nested.overview,
                goal: {
                  ...nested.overview.goal,
                  root: {
                    ...nested.overview.goal.root,
                    id,
                    title: 'Nested fixture goal',
                  },
                },
              },
            },
            history,
          );
        }
        const finished = id === 't-fedcba';
        return withHierarchy(
          {
            ...snapshot,
            overview: {
              ...snapshot.overview,
              goal: {
                ...snapshot.overview.goal,
                root: {
                  ...root,
                  id,
                  status: finished ? 'done' : 'open',
                  title: finished
                    ? 'Completed fixture goal'
                    : id === root.id
                      ? root.title
                      : 'Second fixture goal',
                },
              },
              tasks: snapshot.overview.tasks.map(task => ({
                ...task,
                parent: id,
                status: finished ? 'done' : 'open',
                title: finished ? 'Completed fixture slice' : task.title,
              })),
              progress: finished
                ? {...snapshot.overview.progress, done: 1, open: 0, ready: 0}
                : snapshot.overview.progress,
            },
          },
          history,
        );
      },
    },
    {
      async record(_cwd, id) {
        return {
          project: snapshot.project,
          task: id,
          seq: 9,
          current: 10,
          result: 'Historical fixture result',
          evidence: [
            {slug: 'fixture-report', digest: 'sha256:' + 'a'.repeat(64)},
          ],
          purged: [],
        };
      },
      async page(_cwd, _project, _selection, start) {
        mark(`evidence-page:${start}`);
        return {
          path: '/fixture/report.md',
          state: 'available',
          text:
            start === 1
              ? '# EVIDENCE_PAGE_ONE\nExact retained fixture bytes.'
              : '# EVIDENCE_PAGE_TWO\nEnd of retained fixture.',
          next: start === 1 ? 201 : null,
        };
      },
      async attached() {
        return {slugs: [], next: null};
      },
      async manifest() {
        return {revisions: [], next: null};
      },
    },
    {
      // The attention flow seeds one open request on the fixture goal.
      source: attentionStore(
        process.env.DOTS_FIXTURE_ATTENTION
          ? {requests: [request({goal: 't-123abc'})]}
          : {},
      ).source as AttentionSource,
      tasks: undefined,
    },
  );
  pi.on('session_start', () => mark('start'));
  pi.on('agent_start', () => mark('UNEXPECTED_PROVIDER_TURN'));
  pi.on('input', () => ({action: 'handled'}));
  pi.registerCommand('goal-fixture-theme', {
    description: 'Select a stock theme in the isolated fixture',
    handler: async (name, ctx) => {
      assert.ok(name === 'light' || name === 'dark');
      assert.ok(ctx.ui.setTheme(name).success);
      mark(`fixture-theme:${name}`);
    },
  });
  pi.registerCommand('goal-fixture-exit', {
    description: 'Exit isolated fixture',
    handler: async (_args, ctx) => ctx.shutdown(),
  });
}
