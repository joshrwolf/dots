import {mkdtemp, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {Type, type Static, type TSchema} from 'typebox';
import {Check} from 'typebox/value';
import {DotsError, dots, type DotsOptions} from '@joshrwolf/pi-kit/dots';

/**
 * The `dots attention` client: goal-owned human requests and the decisions
 * recorded from them. The store owns identity, state transitions and
 * validation; every value it prints is checked here before use. Scope is
 * always explicit: an empty goal set reads nothing.
 */

const TaskId = Type.String({pattern: '^t-[a-f0-9]+$', maxLength: 128});
const RequestId = Type.String({pattern: '^r-[a-f0-9]{32}$'});
const DecisionId = Type.String({pattern: '^d-[a-f0-9]{32}$'});
const Project = Type.String({pattern: '^p-[a-f0-9]{32}$'});
const Nullable = <T extends TSchema>(schema: T) =>
  Type.Union([schema, Type.Null()]);
const Seconds = Type.Integer({minimum: 0});
const InputSource = Type.Union([Type.Literal('ui'), Type.Literal('chat')]);
/** Where the human input came from: one line, required by the store. */
const Provenance = Type.String({
  minLength: 1,
  maxLength: 512,
  pattern: '^[^\\r\\n]+$',
});
const Choice = Type.Object({
  id: Type.String({pattern: '^[a-z0-9-]{1,32}$'}),
  label: Type.String(),
  detail: Nullable(Type.String()),
});
const Answer = Type.Object({
  choice: Nullable(Type.String()),
  text: Nullable(Type.String()),
  source: InputSource,
  provenance: Provenance,
  interpretation: Nullable(Type.String()),
  at: Seconds,
  by: Nullable(Type.String()),
});
const Closure = Type.Object({
  kind: Type.Union([
    Type.Literal('incorporated'),
    Type.Literal('withdrawn'),
    Type.Literal('superseded'),
  ]),
  at: Seconds,
  by: Nullable(Type.String()),
  reason: Nullable(Type.String()),
  decision: Nullable(DecisionId),
});
const RequestSchema = Type.Object({
  id: RequestId,
  project: Project,
  goal: TaskId,
  key: Type.String(),
  kind: Type.Union([
    Type.Literal('decision'),
    Type.Literal('authorization'),
    Type.Literal('action'),
  ]),
  title: Type.String(),
  question: Type.String(),
  context: Nullable(Type.String()),
  recommendation: Nullable(Type.String()),
  choices: Type.Array(Choice, {maxItems: 8}),
  tasks: Type.Array(
    Type.Object({id: TaskId, title: Type.String(), status: Type.String()}),
    {maxItems: 32},
  ),
  state: Type.Union([
    Type.Literal('open'),
    Type.Literal('answered'),
    Type.Literal('closed'),
  ]),
  created_at: Seconds,
  created_by: Nullable(Type.String()),
  supersedes: Nullable(RequestId),
  superseded_by: Nullable(RequestId),
  answer: Nullable(Answer),
  closure: Nullable(Closure),
});
const DecisionSchema = Type.Object({
  id: DecisionId,
  project: Project,
  goal: TaskId,
  kind: Type.Union([Type.Literal('mandate'), Type.Literal('decision')]),
  decision: Type.String(),
  human: Nullable(
    Type.Object({
      text: Type.String(),
      source: InputSource,
      provenance: Provenance,
    }),
  ),
  request: Nullable(RequestId),
  at: Seconds,
  by: Nullable(Type.String()),
});
const ScopeError = Type.Object({
  project: Type.String(),
  goal: Nullable(Type.String()),
  error: Type.String(),
});
const GoalHead = {
  project: Project,
  project_name: Type.String(),
  goal: TaskId,
  title: Type.String(),
};
const ListSchema = Type.Object({
  goals: Type.Array(
    Type.Object({
      ...GoalHead,
      status: Type.String({minLength: 1, maxLength: 64}),
      /** The root claim's holder; a claim is not proof its process runs. */
      coordinator: Nullable(Type.String()),
      mandate: Nullable(DecisionSchema),
      requests: Type.Array(RequestSchema),
    }),
  ),
  errors: Type.Array(ScopeError),
});
const DecisionsSchema = Type.Object({
  goals: Type.Array(
    Type.Object({...GoalHead, decisions: Type.Array(DecisionSchema)}),
  ),
  errors: Type.Array(ScopeError),
});
const Shown = Type.Object({request: RequestSchema});
const CoordinatingSchema = Type.Object({
  goals: Type.Array(Type.Object({project: Project, goal: TaskId})),
  now: Seconds,
});

/** An immutable human request. Text fields are writer-controlled. */
export type Request = Static<typeof RequestSchema>;
export type Decision = Static<typeof DecisionSchema>;
export type AttentionList = Static<typeof ListSchema>;
export type GoalAttention = AttentionList['goals'][number];
export type DecisionList = Static<typeof DecisionsSchema>;

/** One goal, exactly: its registered project key and root task. */
export interface GoalRef {
  readonly project: string;
  readonly id: string;
}

/** What a human said, for `answer`. UI answers carry no interpretation. */
export type AnswerContent =
  | {
      readonly source: 'ui';
      readonly choice?: string;
      readonly text?: string;
      readonly provenance: string;
    }
  | {
      readonly source: 'chat';
      readonly choice?: string;
      readonly text: string;
      readonly provenance: string;
      readonly interpretation: string;
    };

/** A decision established in chat, resting on attested human words. */
export interface DecisionContent {
  readonly kind: 'mandate' | 'decision';
  readonly decision: string;
  readonly human: {
    readonly text: string;
    readonly source: 'chat' | 'ui';
    readonly provenance: string;
  };
}

export type DotsRun = (
  args: readonly string[],
  options: DotsOptions,
) => Promise<unknown>;

/** The environment carries the acting runtime (DOTS_AGENT_OWNER). */
export interface Actor {
  readonly env: NodeJS.ProcessEnv;
}

export interface AttentionSource {
  /** Outstanding requests and current mandates for exactly these goals. */
  list(goals: readonly GoalRef[], signal: AbortSignal): Promise<AttentionList>;
  /** Every decision recorded for these goals, oldest first. */
  decisions(
    goals: readonly GoalRef[],
    signal: AbortSignal,
  ): Promise<DecisionList>;
  /** Request commands resolve in the request's own project. */
  show(project: string, request: string, signal: AbortSignal): Promise<Request>;
  answer(
    project: string,
    request: string,
    content: AnswerContent,
    actor: Actor,
    signal: AbortSignal,
  ): Promise<Request>;
  decide(
    goal: GoalRef,
    content: DecisionContent,
    actor: Actor,
    signal: AbortSignal,
  ): Promise<Decision>;
  /**
   * The goals whose open or in-review root `owner` claims, in every
   * registered project: the store's root claims, read fresh each time.
   */
  coordinating(
    owner: string,
    actor: Actor,
    signal: AbortSignal,
  ): Promise<readonly GoalRef[]>;
}

const empty = {goals: [], errors: []};

const scope = (goals: readonly GoalRef[]) =>
  goals.flatMap(goal => ['--goal', `${goal.project}:${goal.id}`]);

function checked<T extends TSchema>(
  schema: T,
  value: unknown,
  what: string,
): Static<T> {
  if (!Check(schema, value))
    throw new Error(`dots ${what} returned an unexpected shape`);
  return value;
}

/** Write private content, run, and always remove it. */
async function withContent<T>(
  content: unknown,
  run: (file: string) => Promise<T>,
): Promise<T> {
  const directory = await mkdtemp(join(tmpdir(), 'dots-attention-'));
  try {
    const file = join(directory, 'content.json');
    await writeFile(file, JSON.stringify(content), {mode: 0o600, flag: 'wx'});
    return await run(file);
  } finally {
    await rm(directory, {recursive: true, force: true});
  }
}

/** A failed command's own message, which names its remedy. */
export function failure(error: unknown): string {
  if (error instanceof DotsError)
    return error.reason === 'invalid-json'
      ? 'dots returned invalid attention JSON'
      : error.message;
  return error instanceof Error ? error.message : String(error);
}

export function dotsAttention(
  run: DotsRun = dots,
  env?: NodeJS.ProcessEnv,
): AttentionSource {
  const read = (args: readonly string[], signal: AbortSignal) =>
    run(args, {signal, ...(env ? {env} : {})});
  return {
    async list(goals, signal) {
      if (!goals.length) return empty;
      return checked(
        ListSchema,
        await read(
          ['attention', 'list', '--scope', 'goals', ...scope(goals)],
          signal,
        ),
        'attention list',
      );
    },
    async decisions(goals, signal) {
      if (!goals.length) return empty;
      return checked(
        DecisionsSchema,
        await read(
          ['attention', 'decisions', '--scope', 'goals', ...scope(goals)],
          signal,
        ),
        'attention decisions',
      );
    },
    async show(project, request, signal) {
      return checked(
        Shown,
        await read(
          ['attention', 'show', request, '--project', project],
          signal,
        ),
        'attention show',
      ).request;
    },
    async answer(project, request, content, actor, signal) {
      const shown = await withContent(content, file =>
        run(
          [
            'attention',
            'answer',
            request,
            '--project',
            project,
            '--content-file',
            file,
          ],
          {
            signal,
            env: actor.env,
          },
        ),
      );
      return checked(Shown, shown, 'attention answer').request;
    },
    async decide(goal, content, actor, signal) {
      const shown = await withContent(content, file =>
        run(
          [
            'attention',
            'decide',
            goal.id,
            '--project',
            goal.project,
            '--content-file',
            file,
          ],
          {signal, env: actor.env},
        ),
      );
      return checked(
        Type.Object({decision: DecisionSchema}),
        shown,
        'attention decide',
      ).decision;
    },
    async coordinating(owner, actor, signal) {
      const listed = checked(
        CoordinatingSchema,
        await run(['task', '--owner', owner, 'goal', 'coordinating'], {
          signal,
          env: actor.env,
        }),
        'task goal coordinating',
      );
      return listed.goals.map(({project, goal}) => ({project, id: goal}));
    },
  };
}
