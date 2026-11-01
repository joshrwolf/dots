import type {GoalView} from '../extensions/goal/present.ts';
import type {WorkerActivitySnapshot} from '../src/ui/worker-activity.ts';
import {
  withHierarchy,
  type GoalSnapshot,
  type GoalTask,
} from '../extensions/goal/source.ts';

// Neutral recorded task states only; no store, CLI, workers or model turns.
const task = (
  id: string,
  title: string,
  fields: Partial<GoalTask> = {},
): GoalTask => ({
  id,
  title,
  status: 'open',
  parent: 't-111111',
  children: 0,
  owner: null,
  result: null,
  result_event: null,
  evidence: [],
  purged_evidence: [],
  references: [],
  branch: null,
  blocked_by: [],
  last_event: null,
  ...fields,
});
export function goalFixtures(phase = 0): GoalView[] {
  if (phase >= 8) return [nestedGoalFixture(phase)];
  const root = task('t-111111', 'Unify the export interface', {
    parent: null,
    children: 4,
  });
  let tasks = [
    task('t-222222', 'Define shared types', {status: 'done'}),
    task('t-333333', 'Update exports', {
      owner: 'fixture-reviewer',
    }),
    task('t-444444', 'Adapt consumers', {blocked_by: ['t-333333']}),
    task('t-555555', 'Document the interface'),
  ];
  if (phase === 1)
    tasks[1] = task('t-333333', 'Resolve compatibility requirements', {
      status: 'held',
    });
  if (phase === 2)
    tasks = tasks.map(task => ({
      ...task,
      status: 'done',
      owner: null,
      blocked_by: [],
    }));
  if (phase === 7) tasks = [];
  const view: GoalView = {
    focus: {id: root.id, project: '/workspace/example'},
    snapshot: {
      project: '/workspace/example',
      now: 100,
      overview: {
        goal: {
          root: {
            ...root,
            children: tasks.length,
            status: phase === 2 ? 'done' : 'open',
          },
          brief: '/fixture/goal.md',
          briefContent: {text: '# Fixture goal'},
        },
        tasks,
        hidden_closed: 0,
        progress: {
          total: tasks.length,
          done: tasks.filter(t => t.status === 'done').length,
          open: tasks.filter(t => t.status === 'open').length,
          held: tasks.filter(t => t.status === 'held').length,
          dropped: 0,
          claimed: tasks.filter(t => t.owner).length,
          ready: tasks.filter(
            t => t.status === 'open' && !t.owner && !t.blocked_by.length,
          ).length,
          review: 0,
        },
      },
    },
    brief: {
      text: '# Shared export interface\n\nPreserve compatibility, make ownership explicit, and verify the public entrypoints.',
      ...(phase === 6
        ? {warning: 'Brief truncated; read /fixture/goal.md for the rest.'}
        : {}),
    },
  };
  if (phase === 4)
    return [
      {...view, error: 'Goal source unavailable. Use /goal refresh to retry.'},
    ];
  if (phase === 5) return [{focus: view.focus}];
  if (phase !== 3) return [view];
  return [
    view,
    ...['Align CLI output', 'Refresh usage examples'].map(
      (title, i): GoalView => {
        const id = i === 0 ? 't-666666' : 't-777777';
        return {
          ...view,
          focus: {...view.focus, id},
          snapshot: {
            ...view.snapshot!,
            overview: {
              ...view.snapshot!.overview,
              goal: {
                root: {...root, id, title},
                brief: '/fixture/other-goal.md',
                briefContent: {text: '# Other fixture goal'},
              },
              tasks: tasks.map(task => ({...task, parent: id})),
            },
          },
        };
      },
    ),
  ];
}

export function goalActivityFixture(
  phase: number,
  frame = 0,
): WorkerActivitySnapshot | undefined {
  if (phase !== 11 && phase !== 12 && phase !== 13) return undefined;
  return {
    frame,
    workers: [
      {
        assignment: {project: '/workspace/example', id: 't-333333'},
        state: phase === 11 ? 'running' : phase === 12 ? 'asking' : 'quiet',
      },
    ],
  };
}

function nestedGoalFixture(phase: number): GoalView {
  const root = task('t-111111', 'Unify the export interface', {
    parent: null,
    children: 2,
  });
  const tasks = [
    task('t-222222', 'Define shared types', {
      status: 'done',
      parent: 't-888888',
    }),
    task('t-888888', 'Core interface', {children: 3}),
    task('t-333333', 'Update exports', {
      parent: 't-888888',
      owner: 'fixture-reviewer',
    }),
    task('t-999999', 'Consumer migration', {parent: 't-888888', children: 1}),
    task('t-444444', 'Adapt consumers', {
      parent: 't-999999',
      blocked_by: ['t-333333'],
    }),
    task('t-aaaaaa', 'Documentation', {children: 3}),
    task('t-bbbbbb', 'Document shared types', {
      parent: 't-aaaaaa',
      status: 'done',
    }),
    task('t-cccccc', 'Resolve documentation policy', {
      parent: 't-aaaaaa',
      status: phase >= 11 ? 'open' : 'held',
    }),
    task('t-555555', 'Document the interface', {parent: 't-aaaaaa'}),
  ];
  const complete: GoalSnapshot = {
    project: '/workspace/example',
    now: 100,
    overview: {
      goal: {
        root,
        brief: '/fixture/goal.md',
        briefContent: {text: '# Fixture goal'},
      },
      tasks,
      hidden_closed: 0,
      progress: {
        total: 6,
        done: 2,
        open: phase >= 11 ? 4 : 3,
        held: phase >= 11 ? 0 : 1,
        dropped: 0,
        claimed: 1,
        ready: phase >= 11 ? 2 : 1,
        review: 0,
      },
    },
  };
  const snapshot =
    phase === 10
      ? {
          ...complete,
          overview: {
            ...complete.overview,
            tasks: tasks.filter(
              t => t.status !== 'done' && t.id !== 't-999999',
            ),
            hidden_closed: 3,
          },
        }
      : withHierarchy(complete, phase === 9);
  return {
    focus: {id: root.id, project: complete.project},
    snapshot,
    history: phase === 9,
    brief: {
      text: '# Shared export interface\n\nTwo workstreams with nested consumer migration. Counts describe recorded leaf states.',
    },
  };
}
