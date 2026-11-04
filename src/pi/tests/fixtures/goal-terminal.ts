// Opt-in neutral fixture: real goal extension/components, no CLI or model calls.
import {appendFileSync} from 'node:fs';
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
        layered: '  [○ Reference fixture slice]',
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
  );
  pi.on('session_start', () => mark('start'));
  pi.on('agent_start', () => mark('UNEXPECTED_PROVIDER_TURN'));
  pi.on('input', () => ({action: 'handled'}));
  pi.registerCommand('goal-fixture-exit', {
    description: 'Exit isolated fixture',
    handler: async (_args, ctx) => ctx.shutdown(),
  });
}
