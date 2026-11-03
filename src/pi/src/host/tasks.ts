import {DOTS, DotsError, dots, type DotsOptions} from '@joshrwolf/pi-kit/dots';
import {Type, type Static, type TSchema} from 'typebox';
import {Check} from 'typebox/value';
import {systemClock, type Clock} from './clock.ts';
import {NdjsonStream, type Launch, type StreamState} from './stream.ts';

/**
 * The companion's one owner of task store observation: the event vocabulary,
 * task lookups, subtree history pages, and `dots task events --follow`
 * streams. The store, project resolution, subtree membership and validation
 * stay with the `dots` CLI; every value it prints is checked here before use.
 */

const TaskId = Type.String({pattern: '^t-[a-f0-9]+$', maxLength: 128});
const Text = Type.Union([Type.String(), Type.Null()]);
const EventSchema = Type.Object({
  seq: Type.Integer({minimum: 1}),
  task: TaskId,
  at: Type.Integer(),
  actor: Text,
  kind: Type.String({minLength: 1, maxLength: 64}),
  detail: Text,
  result: Text,
});
const ErrorLine = Type.Object({error: Type.String()});
const Kinds = Type.Object({
  kinds: Type.Array(Type.String({minLength: 1, maxLength: 64}), {
    minItems: 1,
  }),
});
const Shown = Type.Object({
  project: Type.String({minLength: 1, maxLength: 256}),
  task: Type.Object({
    id: TaskId,
    parent: Type.Union([TaskId, Type.Null()]),
    title: Type.String(),
    status: Type.String({minLength: 1, maxLength: 64}),
  }),
});
const Page = Type.Object({
  events: Type.Array(EventSchema),
  head: Type.Integer({minimum: 0}),
});

/** One history entry. Actor, detail and result are writer-controlled, untrusted text. */
export type TaskEvent = Static<typeof EventSchema>;

export interface TaskInfo {
  /** The registered project key the task belongs to. */
  readonly project: string;
  readonly id: string;
  readonly parent: string | null;
  /** Writer-controlled text. */
  readonly title: string;
  readonly status: string;
}

export interface TaskPage {
  readonly events: readonly TaskEvent[];
  /** The page was full: more events may follow its last one. */
  readonly more: boolean;
}

/** Runs one `dots --json` command; the test seam for one-shot calls. */
export type DotsRun = (
  args: readonly string[],
  options: DotsOptions,
) => Promise<unknown>;

export interface FollowListener {
  /** The next event in the subtree; a respawned follower replays from `since`. */
  event(event: TaskEvent): void;
  state(state: StreamState): void;
}

export interface TasksOptions {
  readonly run?: DotsRun;
  readonly launch?: Launch;
  readonly clock?: Clock;
  /** The catalog follows `XDG_DATA_HOME` in this environment. */
  readonly env?: NodeJS.ProcessEnv;
}

/**
 * Events per history page to start with. Events carry results of up to 24 KiB,
 * so a page can outgrow the client's output cap; it then halves.
 */
const PAGE = 100;

const is = <T extends TSchema>(schema: T, value: unknown): value is Static<T> =>
  Check(schema, value);

/** A stream line: an event, the follower's storage failure, or nothing observers use. */
export function parseTaskLine(
  value: unknown,
): {event: TaskEvent} | {error: string} | undefined {
  if (is(EventSchema, value)) return {event: value};
  if (is(ErrorLine, value)) return {error: value.error};
  return undefined;
}

export class Tasks {
  private readonly run: DotsRun;
  private readonly launch: Launch | undefined;
  private readonly clock: Clock;
  private readonly env: NodeJS.ProcessEnv | undefined;
  private vocabulary: Promise<readonly string[]> | undefined;
  /** Events per history page; shrinks when a page exceeds the output cap. */
  private page = PAGE;
  private readonly streams = new Set<NdjsonStream>();
  private readonly closing = new Set<Promise<void>>();
  private readonly calls = new Set<{
    controller: AbortController;
    done: Promise<unknown>;
  }>();

  constructor(options: TasksOptions = {}) {
    this.run = options.run ?? dots;
    this.launch = options.launch;
    this.clock = options.clock ?? systemClock;
    this.env = options.env;
  }

  private async call<T extends TSchema>(
    args: readonly string[],
    schema: T,
    options: Pick<DotsOptions, 'directory'> = {},
  ): Promise<Static<T>> {
    const controller = new AbortController();
    const done = this.run(args, {
      ...options,
      signal: controller.signal,
      ...(this.env ? {env: this.env} : {}),
    });
    const call = {controller, done};
    this.calls.add(call);
    let value: unknown;
    try {
      value = await done;
    } finally {
      this.calls.delete(call);
    }
    if (!is(schema, value))
      throw new Error(
        `dots ${args.slice(0, 2).join(' ')} returned an unexpected shape`,
      );
    return value;
  }

  /** Every kind an event can have, as this CLI defines them. */
  kinds(): Promise<readonly string[]> {
    this.vocabulary ??= this.call(['task', 'event-kinds'], Kinds).then(
      value => value.kinds,
      (error: unknown) => {
        this.vocabulary = undefined;
        throw error;
      },
    );
    return this.vocabulary;
  }

  /** One task, in a named project or the project of a checkout directory. */
  async task(
    id: string,
    where: {readonly project: string} | {readonly directory: string},
  ): Promise<TaskInfo> {
    const {project, task} = await this.call(
      [
        'task',
        'show',
        id,
        ...('project' in where ? ['--project', where.project] : []),
      ],
      Shown,
      'directory' in where ? {directory: where.directory} : {},
    );
    return {
      project,
      id: task.id,
      parent: task.parent,
      title: task.title,
      status: task.status,
    };
  }

  /** One page of the history of a task and its descendants after `since`, oldest first. */
  async events(
    project: string,
    root: string,
    since: number,
  ): Promise<TaskPage> {
    for (;;) {
      const limit = this.page;
      try {
        const {events} = await this.call(
          [
            'task',
            'events',
            '--project',
            project,
            '--under',
            root,
            '--since',
            String(since),
            '--limit',
            String(limit),
          ],
          Page,
        );
        return {events, more: events.length >= limit};
      } catch (error) {
        if (limit === 1 || !overflowed(error)) throw error;
        this.page = Math.min(this.page, Math.floor(limit / 2));
      }
    }
  }

  /** The seq of a project's newest event, or 0 before its first. */
  async head(project: string): Promise<number> {
    const {head} = await this.call(
      ['task', 'events', '--project', project, '--limit', '0'],
      Page,
    );
    return head;
  }

  /**
   * Follow the events on a task and its descendants after `since`, evaluated
   * at each spawn so a reconnect resumes from the caller's cursor. The store
   * judges membership by the tree at each read. A storage failure's error
   * line becomes the disconnected reason. Stopping ends the follower's process.
   */
  follow(
    project: string,
    root: string,
    since: () => number,
    listener: FollowListener,
  ): () => void {
    const stream = new NdjsonStream({
      file: DOTS,
      args: () => [
        '--json',
        'task',
        'events',
        '--follow',
        '--project',
        project,
        '--under',
        root,
        '--since',
        String(since()),
      ],
      clock: this.clock,
      ...(this.launch ? {launch: this.launch} : {}),
      ...(this.env ? {env: this.env} : {}),
    });
    this.streams.add(stream);
    let failure: string | undefined;
    const stop = stream.subscribe({
      open: () => {
        failure = undefined;
      },
      line: value => {
        const line = parseTaskLine(value);
        if (!line) return;
        if ('error' in line) failure = line.error;
        else listener.event(line.event);
      },
      state: state =>
        listener.state(
          state.status === 'disconnected' && failure !== undefined
            ? {...state, error: failure}
            : state,
        ),
    });
    let stopped = false;
    return () => {
      if (stopped) return;
      stopped = true;
      stop();
      this.streams.delete(stream);
      const closed = stream.close();
      this.closing.add(closed);
      const forget = () => this.closing.delete(closed);
      closed.then(forget, forget);
    };
  }

  /** Stop every follower and call; resolves once their processes have exited. */
  async close(): Promise<void> {
    const streams = [...this.streams];
    this.streams.clear();
    const calls = [...this.calls];
    for (const call of calls) call.controller.abort();
    await Promise.allSettled([
      ...streams.map(stream => stream.close()),
      ...this.closing,
      ...calls.map(call => call.done),
    ]);
  }
}

/** Whether a call failed only because its output exceeded the client's cap. */
function overflowed(error: unknown): boolean {
  return (
    error instanceof DotsError &&
    error.cause instanceof Error &&
    'code' in error.cause &&
    error.cause.code === 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER'
  );
}
