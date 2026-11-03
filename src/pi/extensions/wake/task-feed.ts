import type {Clock} from '../../src/host/clock.ts';
import type {StreamState} from '../../src/host/stream.ts';
import type {Tasks, TaskEvent} from '../../src/host/tasks.ts';

// Task store observation for task wakes: one follower per project, shared by
// that project's wakes and started from their lowest cursor, and the parentage
// that decides which events belong to a watched tree. Events reach wakes one
// at a time in seq order; each wake skips seqs at or below its own cursor, so
// a replay after a reconnect or restart neither skips nor repeats one.

/** What task wakes need from the task store; `Tasks` in production, a fake in tests. */
export type TaskSource = Pick<
  Tasks,
  'kinds' | 'task' | 'events' | 'head' | 'follow' | 'close'
>;

export interface Watched {
  readonly trigger: {
    readonly id: string;
    readonly project: string;
    readonly cursor: number;
  };
}

export interface TaskHost<W extends Watched> {
  /** The project's wakes that observe live events now; empty once the runtime ends. */
  wakes(project: string): readonly W[];
  /** One event for one wake whose cursor is below it. */
  observe(wake: W, event: TaskEvent, inTree: boolean): void;
  /** Connection state changed. */
  changed(): void;
}

interface Feed {
  readonly project: string;
  stop: (() => void) | undefined;
  /** Received and not yet delivered, in seq order. */
  queue: TaskEvent[];
  draining: boolean;
  /** The highest seq delivered on this feed. */
  drained: number;
  state: StreamState | undefined;
  /** Why delivery stopped, until an event is delivered again. */
  error: string | undefined;
  retry: unknown;
}

/** Ancestry deeper than this is treated as outside every watched tree. */
const MAX_DEPTH = 256;

export class TaskFeeds<W extends Watched> {
  private readonly feeds = new Map<string, Feed>();
  /** Each project's known parent links, kept current by `updated` events. */
  private readonly parents = new Map<string, Map<string, string | null>>();
  private readonly work = new Set<Promise<void>>();
  private readonly source: TaskSource;
  private readonly host: TaskHost<W>;
  private readonly clock: Clock;
  private readonly retryMs: number;
  private epoch = 0;

  constructor(
    source: TaskSource,
    host: TaskHost<W>,
    clock: Clock,
    retryMs = 5_000,
  ) {
    this.source = source;
    this.host = host;
    this.clock = clock;
    this.retryMs = retryMs;
  }

  /** Follow exactly these projects. */
  sync(projects: ReadonlySet<string>): void {
    for (const [project, feed] of this.feeds)
      if (!projects.has(project)) {
        this.halt(feed);
        this.feeds.delete(project);
      }
    for (const project of projects) {
      if (this.feeds.has(project)) continue;
      const feed: Feed = {
        project,
        stop: undefined,
        queue: [],
        draining: false,
        drained: 0,
        state: undefined,
        error: undefined,
        retry: undefined,
      };
      this.feeds.set(project, feed);
      this.start(feed);
    }
  }

  /**
   * A wake joined a running feed. Events the feed delivered before it joined
   * and after its cursor would be lost, so the feed replays from the lowest
   * cursor.
   */
  joined(wake: W): void {
    const feed = this.feeds.get(wake.trigger.project);
    if (!feed?.stop || feed.drained <= wake.trigger.cursor) return;
    this.halt(feed);
    this.start(feed);
  }

  /** Why a project cannot be observed right now, or undefined while it can. */
  disconnected(project: string): string | undefined {
    const feed = this.feeds.get(project);
    if (!feed) return undefined;
    if (feed.error !== undefined) return feed.error;
    return feed.state?.status === 'disconnected'
      ? feed.state.error || 'disconnected'
      : undefined;
  }

  /**
   * Deliver the history after a wake's cursor up to its deadline, for a wake
   * whose deadline passed while nothing watched. Stops at the first event
   * after the deadline; rejects when the store cannot be read.
   */
  expire(wake: W, deadline: number): Promise<void> {
    const epoch = this.epoch;
    const project = wake.trigger.project;
    // Past its deadline the wake observes no live events; deliver to it alone.
    const only = () => (epoch === this.epoch ? [wake] : []);
    const done = (async () => {
      let since = wake.trigger.cursor;
      for (;;) {
        const page = await this.source.events(project, since);
        for (const event of page.events) {
          if (event.at * 1000 > deadline) return;
          await this.deliver(project, event, only);
          since = event.seq;
        }
        if (!page.more || epoch !== this.epoch) return;
      }
    })();
    this.track(done);
    return done;
  }

  /** Stop every feed and forget parentage; work in flight publishes nothing. */
  stop(): void {
    this.epoch++;
    for (const feed of this.feeds.values()) this.halt(feed);
    this.feeds.clear();
    this.parents.clear();
  }

  /** Stop, then wait for deliveries in flight to settle. */
  async close(): Promise<void> {
    this.stop();
    await Promise.allSettled([...this.work]);
  }

  private track(work: Promise<void>): void {
    this.work.add(work);
    const forget = () => this.work.delete(work);
    work.then(forget, forget);
  }

  private current(feed: Feed): boolean {
    return this.feeds.get(feed.project) === feed;
  }

  private start(feed: Feed): void {
    const {project} = feed;
    feed.stop = this.source.follow(
      project,
      () => {
        const cursors = this.host.wakes(project).map(w => w.trigger.cursor);
        // Without wakes the feed is about to stop; any valid cursor will do.
        return cursors.length ? Math.min(...cursors) : 0;
      },
      {
        // A new follower replays everything after the lowest cursor.
        open: () => {
          feed.queue = [];
        },
        event: event => {
          if (!this.current(feed)) return;
          feed.queue.push(event);
          this.drain(feed);
        },
        state: state => {
          if (!this.current(feed)) return;
          feed.state = state;
          this.host.changed();
        },
      },
    );
  }

  private halt(feed: Feed): void {
    if (feed.retry !== undefined) this.clock.clearTimeout(feed.retry);
    feed.retry = undefined;
    feed.stop?.();
    feed.stop = undefined;
    feed.queue = [];
  }

  private drain(feed: Feed): void {
    if (feed.draining) return;
    feed.draining = true;
    const select = () => this.host.wakes(feed.project);
    this.track(
      (async () => {
        try {
          for (
            let event = feed.queue.shift();
            event && this.current(feed);
            event = feed.queue.shift()
          ) {
            await this.deliver(feed.project, event, select);
            feed.drained = Math.max(feed.drained, event.seq);
            feed.error = undefined;
          }
        } catch (error) {
          if (!this.current(feed)) return;
          // Parentage is unknown, so the event cannot be judged: replay
          // from the cursors once the store answers again.
          feed.error = `could not read the task store: ${error instanceof Error ? error.message : String(error)}`;
          this.halt(feed);
          feed.retry = this.clock.setTimeout(() => {
            feed.retry = undefined;
            if (this.current(feed)) this.start(feed);
          }, this.retryMs);
          this.host.changed();
        } finally {
          feed.draining = false;
        }
      })(),
    );
  }

  /** Judge one event for the selected wakes whose cursor is below it. */
  private async deliver(
    project: string,
    event: TaskEvent,
    select: () => readonly W[],
  ): Promise<void> {
    if (!select().some(wake => event.seq > wake.trigger.cursor)) return;
    // Only an update can move a task to another parent.
    if (event.kind === 'updated') this.links(project).delete(event.task);
    const lineage = await this.lineage(project, event.task);
    for (const wake of select())
      if (event.seq > wake.trigger.cursor)
        this.host.observe(wake, event, lineage.has(wake.trigger.id));
  }

  private links(project: string): Map<string, string | null> {
    let links = this.parents.get(project);
    if (!links) {
      links = new Map();
      this.parents.set(project, links);
    }
    return links;
  }

  /** The task and its ancestors, learning parents through the CLI once each. */
  private async lineage(project: string, id: string): Promise<Set<string>> {
    const epoch = this.epoch;
    const lineage = new Set<string>();
    let current: string | null = id;
    while (current !== null && !lineage.has(current)) {
      if (lineage.size >= MAX_DEPTH) return new Set();
      lineage.add(current);
      const links = this.links(project);
      let parent = links.get(current);
      if (parent === undefined) {
        parent = (await this.source.task(current, {project})).parent;
        if (epoch === this.epoch) this.links(project).set(current, parent);
      }
      current = parent;
    }
    return lineage;
  }
}
