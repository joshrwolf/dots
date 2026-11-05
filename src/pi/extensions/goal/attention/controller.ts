import type {TurnMessage} from '../../../src/host/delivery.ts';
import {agentEnv, type Agent} from '../../../src/host/agent.ts';
import type {AttentionSignal} from '../../../src/host/attention.ts';
import {
  attentionModel,
  answerText,
  needsAnswer,
  type AttentionModel,
  type ItemState,
} from './model.ts';
import type {Tasks, TaskEvent} from '../../../src/host/tasks.ts';
import type {AttentionFacts} from './context.ts';
import type {Reply} from './panel.ts';
import {
  failure,
  type AttentionList,
  type DecisionList,
  type AttentionSource,
  type GoalRef,
  type Request,
} from './source.ts';

/** A delivered answer notice; on the branch, it is also the delivery receipt. */
export const ANSWER_MESSAGE = 'dots.attention.answer';

export interface AttentionHost {
  readonly source: AttentionSource;
  /** Native turn delivery: holds the message until the session is idle. */
  readonly deliver: (
    compose: () => TurnMessage | undefined,
    delivered?: () => void,
  ) => void;
  /** The current branch, for delivery receipts. */
  readonly branch: () => readonly unknown[];
  /** Repaint views from `model()`. Never called during render. */
  readonly changed: () => void;
  /** Publish semantic attention; called only when it changes. */
  readonly signal: (signal: AttentionSignal) => void;
  /** Shared task events, so answers saved elsewhere arrive without polling. */
  readonly tasks?: Pick<Tasks, 'follow' | 'head'>;
  /** False for managed workers, which never coordinate; skips that read. */
  readonly coordinates?: boolean;
}

const refKey = (goal: GoalRef) => `${goal.project}:${goal.id}`;
const unique = (goals: readonly GoalRef[]) => [
  ...new Map(goals.map(goal => [refKey(goal), goal])).values(),
];

/** Events that change a goal's requests, decisions or coordination. */
const relevant = (event: TaskEvent, root: string) =>
  event.task === root ||
  event.kind.startsWith('request-') ||
  event.kind === 'decision-recorded';

/** Request IDs that answer notices on `branch` already delivered. */
export function deliveredAnswers(branch: readonly unknown[]): Set<string> {
  const told = new Set<string>();
  for (const entry of branch) {
    if (!entry || typeof entry !== 'object') continue;
    const record = entry as {
      type?: unknown;
      customType?: unknown;
      details?: {requests?: unknown};
    };
    if (
      record.type !== 'custom_message' ||
      record.customType !== ANSWER_MESSAGE
    )
      continue;
    const ids = record.details?.requests;
    if (Array.isArray(ids))
      for (const id of ids) if (typeof id === 'string') told.add(id);
  }
  return told;
}

function answerNotice(requests: readonly Request[]): TurnMessage {
  const body = requests.map(request => {
    const answer = request.answer!;
    return [
      `Request ${request.id} (goal ${request.goal}, project ${request.project}): ${request.title}`,
      `Question: ${request.question}`,
      `Answer (${answer.source}, ${answer.provenance}): ${answerText(request)}`,
      ...(answer.interpretation
        ? [`Recorded interpretation: ${answer.interpretation}`]
        : []),
    ].join('\n');
  });
  return {
    customType: ANSWER_MESSAGE,
    content: [
      `A person answered ${requests.length === 1 ? 'a request' : `${requests.length} requests`} on a goal this session coordinates.`,
      'Incorporate each answer with `dots attention incorporate R --content-file F` before continuing the work it holds; an answer does not resume a paused goal.',
      '',
      body.join('\n\n'),
    ].join('\n'),
    display: true,
    details: {requests: requests.map(request => request.id)},
  };
}

/**
 * Attention for this session's goals: exactly the goals it follows plus the
 * goals the store says it coordinates. Coordination is the store's root
 * claims, read fresh on every refresh, so unfollowing never hides a goal this
 * session is responsible for, and pausing, releasing or handing it off removes
 * it. The popup, indicator, context and drafts show nothing else; no follows
 * and no coordination show nothing. Reads are generation-guarded, so a late
 * reply never overwrites a newer scope.
 *
 * Saving an answer persists it before the view says so. Only the goal's
 * current coordinator, by a fresh read, is told an answer to continue with,
 * and never while the goal is held. Delivery notices are their own
 * branch-local receipts; a receipt only says this branch was told, never that
 * the answer was incorporated.
 */
export class AttentionController {
  /** Unsent answer text by request; outlives views, not the runtime. */
  readonly drafts = new Map<string, string>();
  private readonly host: AttentionHost;
  /** Explicitly followed goals. */
  private view: readonly GoalRef[] = [];
  /** Followed and coordinated goals, as last read. */
  private list: AttentionList | undefined;
  private readFailure: string | undefined;
  /** The coordination read failed: responsibilities are unknown, not none. */
  private dutyFailure: string | undefined;
  /**
   * Goals the last successful coordination read found this runtime
   * coordinating; a failed read keeps them shown but acts on none.
   */
  private duties: readonly GoalRef[] = [];
  private readonly local = new Map<
    string,
    {state: ItemState; request: Request; goal: string}
  >();
  private agent: Agent | undefined;
  private reads = 0;
  private reading: AbortController | undefined;
  private readonly lifetime = new AbortController();
  /** Handed to Pi but not yet seen on the branch. */
  private sent = new Set<string>();
  private signalled = '[]';
  /** The last signal's requests, by goal, for goals a later read misses. */
  private alerted: readonly {
    goal: string;
    at: number;
    entry: {id: string; title: string};
  }[] = [];
  /** Per goal: stop its stream; `ready` settles once its cursor is known. */
  private readonly follows = new Map<
    string,
    {readonly stop: () => void; readonly ready: Promise<void>}
  >();
  /** Answered requests that mandates rest on; immutable once answered. */
  private readonly sources = new Map<string, Request>();

  constructor(host: AttentionHost) {
    this.host = host;
  }

  /** Bind the runtime's actor and forget a previous branch's deliveries. */
  bind(agent: Agent | undefined): void {
    this.agent = agent;
    this.sent = new Set();
  }

  get current(): AttentionList | undefined {
    return this.list;
  }

  /** The project of an outstanding request on this session's goals, as last read. */
  projectOf(request: string): string | undefined {
    return this.viewed()
      ?.goals.flatMap(goal => goal.requests)
      .find(item => item.id === request)?.project;
  }

  /** Followed and coordinated goals: the session's effective scope. */
  goals(): readonly GoalRef[] {
    return unique([...this.view, ...this.duties]);
  }

  /** The last read, narrowed to exactly the current scope. */
  private viewed(): AttentionList | undefined {
    const list = this.list;
    if (!list) return undefined;
    const goals = this.goals();
    const keys = new Set(goals.map(refKey));
    const projects = new Set(goals.map(goal => goal.project));
    return {
      ...list,
      goals: list.goals.filter(goal =>
        keys.has(refKey({project: goal.project, id: goal.goal})),
      ),
      errors: list.errors.filter(error =>
        error.goal
          ? keys.has(refKey({project: error.project, id: error.goal}))
          : projects.has(error.project),
      ),
    };
  }

  model(): AttentionModel {
    const model = attentionModel(this.viewed(), this.agent?.owner, this.local);
    const unavailable = [
      ...model.unavailable,
      ...(this.readFailure ? [this.readFailure] : []),
      ...(this.dutyFailure
        ? [
            `Goals this session coordinates could not be checked: ${this.dutyFailure}`,
          ]
        : []),
    ];
    return {...model, unavailable};
  }

  /** Replace the followed goals; requests leaving the scope leave with their local state. */
  setScope(goals: readonly GoalRef[]): Promise<void> {
    const next = unique(goals);
    const same =
      next.length === this.view.length &&
      next.every((goal, at) => refKey(goal) === refKey(this.view[at]!));
    this.view = next;
    if (same && this.list) return Promise.resolve();
    this.prune();
    return this.refresh();
  }

  private prune(): void {
    const kept = new Set(this.goals().map(refKey));
    for (const [id, item] of this.local)
      if (!kept.has(`${item.request.project}:${item.request.goal}`))
        this.local.delete(id);
  }

  /** Stream exactly the goals read. */
  private stream(goals: readonly GoalRef[]): void {
    const kept = new Set(goals.map(refKey));
    for (const [goal, follow] of this.follows)
      if (!kept.has(goal)) {
        follow.stop();
        this.follows.delete(goal);
      }
    for (const goal of goals)
      if (!this.follows.has(refKey(goal))) this.follow(refKey(goal), goal);
  }

  /**
   * Read responsibilities, then the followed and coordinated goals together;
   * equivalent results neither repaint nor signal.
   */
  async refresh(): Promise<void> {
    this.reading?.abort();
    const epoch = ++this.reads;
    const controller = new AbortController();
    this.reading = controller;
    const state = () =>
      JSON.stringify([
        this.list,
        this.readFailure,
        this.dutyFailure,
        this.duties,
      ]);
    const before = state();
    const agent = this.agent;
    let duties: readonly GoalRef[] = [];
    let dutyFailure: string | undefined;
    if (agent && this.host.coordinates !== false)
      try {
        duties = await this.host.source.coordinating(
          agent.owner,
          {env: agentEnv(agent)},
          controller.signal,
        );
      } catch (error) {
        dutyFailure = failure(error);
      }
    if (epoch !== this.reads || this.lifetime.signal.aborted) return;
    this.dutyFailure = dutyFailure;
    if (dutyFailure === undefined) {
      this.duties = duties;
      this.prune();
    }
    const goals = this.goals();
    this.stream(goals);
    // The cursor precedes the snapshot, so nothing between them is missed.
    await Promise.all(goals.map(goal => this.follows.get(refKey(goal))?.ready));
    if (epoch !== this.reads || this.lifetime.signal.aborted) return;
    if (!goals.length) {
      this.reading = undefined;
      this.list = undefined;
      this.readFailure = undefined;
      this.publish();
      if (state() !== before) this.host.changed();
      return;
    }
    try {
      const list = await this.host.source.list(goals, controller.signal);
      if (epoch !== this.reads || this.lifetime.signal.aborted) return;
      this.retain(this.list, list);
      this.list = list;
      this.readFailure = undefined;
    } catch (error) {
      if (epoch !== this.reads || this.lifetime.signal.aborted) return;
      // A failed read is not evidence that nothing is outstanding.
      this.readFailure = `Attention unavailable: ${failure(error)}`;
    } finally {
      if (this.reading === controller) this.reading = undefined;
    }
    this.publish();
    this.deliverAnswers();
    if (state() !== before) this.host.changed();
  }

  /**
   * Follow one goal from the store's head, read before the next snapshot;
   * a reconnect replays from the cursor.
   */
  private follow(key: string, goal: GoalRef): void {
    const tasks = this.host.tasks;
    if (!tasks) return;
    let stop: (() => void) | undefined;
    let stopped = false;
    const ready = tasks.head(goal.project).then(
      head => {
        if (stopped || this.lifetime.signal.aborted) return;
        let cursor = head;
        let lost = false;
        stop = tasks.follow(goal.project, goal.id, () => cursor, {
          event: event => {
            if (event.seq <= cursor) return;
            cursor = event.seq;
            if (relevant(event, goal.id)) this.track(this.refresh());
          },
          state: state => {
            if (state.status === 'disconnected') lost = true;
            else if (state.status === 'connected' && lost) {
              lost = false;
              this.track(this.refresh());
            }
          },
        });
      },
      // Without a cursor, refreshes on runs and explicit reads still apply.
      () => {
        if (this.follows.get(key)?.ready === ready) this.follows.delete(key);
      },
    );
    this.follows.set(key, {
      stop: () => {
        stopped = true;
        stop?.();
      },
      ready,
    });
  }

  /**
   * A drafted request that left its goal's outstanding list stays with its
   * draft as a local outcome: replaced when the read lists its replacement,
   * otherwise no longer outstanding. It never retargets; the person
   * dismisses it. A goal the read missed or that left scope keeps nothing.
   */
  private retain(previous: AttentionList | undefined, next: AttentionList) {
    for (const goal of previous?.goals ?? []) {
      const now = next.goals.find(
        item => item.project === goal.project && item.goal === goal.goal,
      );
      if (!now) continue;
      for (const request of goal.requests) {
        const kept = this.local.get(request.id)?.state.kind;
        if (
          !this.drafts.get(request.id)?.trim() ||
          (kept && kept !== 'error') ||
          now.requests.some(item => item.id === request.id)
        )
          continue;
        const replacement = now.requests.find(
          item => item.supersedes === request.id,
        );
        this.local.set(request.id, {
          request,
          goal: goal.title,
          state: replacement
            ? {kind: 'stale', replacement}
            : {kind: 'closed', message: 'No longer outstanding.'},
        });
      }
    }
  }

  private find(id: string): {request: Request; goal: string} | undefined {
    for (const goal of this.list?.goals ?? []) {
      const request = goal.requests.find(item => item.id === id);
      if (request) return {request, goal: goal.title};
    }
    const kept = this.local.get(id);
    return kept && {request: kept.request, goal: kept.goal};
  }

  /** Persist `reply` for exactly `id`, as the popup's person, then refresh. */
  async answer(id: string, reply: Reply): Promise<void> {
    const found = this.find(id);
    const agent = this.agent;
    if (!found || !agent) return;
    this.local.set(id, {...found, state: {kind: 'saving'}});
    this.host.changed();
    const content = {
      source: 'ui' as const,
      ...reply,
      provenance: `pi session ${agent.session} · goal popup`,
    };
    try {
      await this.host.source.answer(
        found.request.project,
        id,
        content,
        {env: agentEnv(agent)},
        this.lifetime.signal,
      );
      if (this.lifetime.signal.aborted) return;
      this.local.delete(id);
      this.drafts.delete(id);
    } catch (error) {
      if (this.lifetime.signal.aborted) return;
      const state =
        (await this.classify(found.request.project, id).catch(
          () => undefined,
        )) ?? ({kind: 'error', message: failure(error)} as const);
      this.local.set(id, {...found, state});
    }
    this.host.changed();
    await this.refresh();
  }

  /**
   * Why an answer did not land, from the request's current record: still
   * open is an operational failure (undefined); replaced keeps the draft
   * beside the immutable replacement; anything else changed elsewhere.
   */
  private async classify(
    project: string,
    id: string,
  ): Promise<ItemState | undefined> {
    const signal = this.lifetime.signal;
    const shown = await this.host.source.show(project, id, signal);
    if (shown.state === 'open') return undefined;
    if (shown.superseded_by)
      return {
        kind: 'stale',
        replacement: await this.host.source
          .show(project, shown.superseded_by, signal)
          .catch(() => undefined),
      };
    if (shown.state === 'answered')
      return {
        kind: 'closed',
        message: `Already answered elsewhere: ${answerText(shown)}`,
      };
    return {
      kind: 'closed',
      message:
        shown.closure?.kind === 'withdrawn'
          ? 'The coordinator withdrew this request.'
          : 'This request was closed.',
    };
  }

  /** Forget a local outcome and its draft; stored records are unaffected. */
  dismiss(id: string): void {
    this.local.delete(id);
    this.drafts.delete(id);
    this.host.changed();
  }

  /** Goals this runtime coordinates and may continue: a fresh read, not held. */
  private responsible() {
    const owner = this.agent?.owner;
    if (!owner || this.readFailure) return [];
    return (this.list?.goals ?? []).filter(
      goal => goal.coordinator === owner && goal.status !== 'held',
    );
  }

  /**
   * Only the responsible coordinator surfaces native attention, once per
   * change. A failed read leaves the last signal: unknown is not nothing.
   */
  private publish(): void {
    if (this.readFailure || this.dutyFailure) return;
    const errors = this.list?.errors ?? [];
    // A goal or project the read missed keeps what it last alerted.
    const unread = (goal: string) =>
      errors.some(error =>
        error.goal
          ? refKey({project: error.project, id: error.goal}) === goal
          : goal.startsWith(`${error.project}:`),
      );
    const fresh = this.responsible().flatMap(goal => {
      const key = refKey({project: goal.project, id: goal.goal});
      return goal.requests
        .filter(request => request.state === 'open')
        .map(request => ({
          goal: key,
          at: request.created_at,
          entry: {id: request.id, title: request.title},
        }));
    });
    this.alerted = [
      ...fresh,
      ...this.alerted.filter(alert => unread(alert.goal)),
    ].sort((a, b) => a.at - b.at || a.entry.id.localeCompare(b.entry.id));
    const requests = this.alerted.map(alert => alert.entry);
    const key = JSON.stringify(requests);
    if (key === this.signalled) return;
    this.signalled = key;
    this.host.signal({requests});
  }

  /** Tell the responsible coordinator, never while responsibility is unknown. */
  private deliverAnswers(): void {
    if (this.dutyFailure) return;
    const told = deliveredAnswers(this.host.branch());
    for (const id of told) this.sent.delete(id);
    const answered = this.responsible()
      .flatMap(goal => goal.requests)
      .filter(
        request =>
          request.state === 'answered' &&
          request.answer &&
          !told.has(request.id) &&
          !this.sent.has(request.id),
      );
    if (!answered.length) return;
    for (const request of answered) this.sent.add(request.id);
    // Revalidated when Pi takes it: a pause, release or handoff observed
    // while it was held declines it, and a later coordinator is told instead.
    this.host.deliver(() => {
      const current = new Map(
        (this.dutyFailure ? [] : this.responsible())
          .flatMap(goal => goal.requests)
          .filter(request => request.state === 'answered' && request.answer)
          .map(request => [request.id, request]),
      );
      const still = answered.flatMap(request => {
        const fresh = current.get(request.id);
        if (!fresh) this.sent.delete(request.id);
        return fresh ? [fresh] : [];
      });
      return still.length ? answerNotice(still) : undefined;
    });
  }

  /**
   * Fresh facts for each goal's context section: requests and mandates,
   * decisions, and the request a request-linked mandate rests on. Requests
   * are immutable once answered, so resolved sources are cached.
   */
  async facts(): Promise<Map<string, AttentionFacts>> {
    await this.refresh();
    const result = new Map<string, AttentionFacts>();
    const goals = this.goals();
    if (!goals.length) return result;
    let decisions: DecisionList | undefined;
    let decisionFailure: string | undefined;
    try {
      decisions = await this.host.source.decisions(goals, this.lifetime.signal);
    } catch (error) {
      decisionFailure = failure(error);
    }
    for (const ref of goals) {
      const key = `${ref.project}:${ref.id}`;
      const goal = this.list?.goals.find(
        item => item.project === ref.project && item.goal === ref.id,
      );
      const error = this.list?.errors.find(
        item =>
          item.project === ref.project && (item.goal ?? ref.id) === ref.id,
      );
      const recorded = decisions?.goals.find(
        item => item.project === ref.project && item.goal === ref.id,
      );
      const linked = goal?.mandate?.human ? undefined : goal?.mandate?.request;
      let mandateRequest = linked ? this.sources.get(linked) : undefined;
      if (linked && !mandateRequest)
        mandateRequest = await this.host.source
          .show(ref.project, linked, this.lifetime.signal)
          .then(request => {
            if (request.answer) this.sources.set(linked, request);
            return request;
          })
          .catch(() => undefined);
      result.set(key, {
        goal,
        decisions: recorded?.decisions,
        mandateRequest,
        failure:
          this.readFailure ??
          error?.error ??
          (goal ? undefined : decisionFailure),
      });
    }
    return result;
  }

  /**
   * The goals the last successful read found this runtime coordinating, and
   * why the latest read failed, if it did.
   */
  coordinated(): {
    readonly goals: readonly GoalRef[];
    readonly failure: string | undefined;
  } {
    return {goals: this.duties, failure: this.dutyFailure};
  }

  /** Run work in the background; an unexpected failure shows, never vanishes. */
  track(work: Promise<void>): void {
    work.catch((error: unknown) => {
      this.readFailure = `Attention failed: ${failure(error)}`;
      this.host.changed();
    });
  }

  /** Requests that still need a person, for the quiet indicator. */
  waiting(): number {
    return this.model().items.filter(needsAnswer).length;
  }

  close(): void {
    for (const follow of this.follows.values()) follow.stop();
    this.follows.clear();
    this.lifetime.abort();
    this.reading?.abort();
    this.reads++;
  }
}
