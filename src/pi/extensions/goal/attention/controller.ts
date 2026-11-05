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
    compose: () => TurnMessage,
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
}

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
 * Attention for the goals this session follows. Scope is exactly the followed
 * (project, goal) pairs; with none, nothing is read or shown. Reads are
 * generation-guarded, so a late reply never overwrites a newer scope.
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
  private goals: readonly GoalRef[] = [];
  private list: AttentionList | undefined;
  private readFailure: string | undefined;
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
  private readonly follows = new Map<string, () => void>();
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

  /** The project of an outstanding request in scope, as last read. */
  projectOf(request: string): string | undefined {
    return this.list?.goals
      .flatMap(goal => goal.requests)
      .find(item => item.id === request)?.project;
  }

  model(): AttentionModel {
    const model = attentionModel(this.list, this.agent?.owner, this.local);
    return this.readFailure
      ? {...model, unavailable: [...model.unavailable, this.readFailure]}
      : model;
  }

  /** Replace the scope; requests outside it leave with their local state. */
  setScope(goals: readonly GoalRef[]): Promise<void> {
    const key = (goal: GoalRef) => `${goal.project}:${goal.id}`;
    const next = [...new Map(goals.map(goal => [key(goal), goal])).values()];
    const same =
      next.length === this.goals.length &&
      next.every((goal, at) => key(goal) === key(this.goals[at]!));
    this.goals = next;
    if (same && this.list) return Promise.resolve();
    const kept = new Set(next.map(key));
    for (const [id, item] of this.local)
      if (!kept.has(`${item.request.project}:${item.request.goal}`))
        this.local.delete(id);
    for (const [goal, stop] of this.follows)
      if (!kept.has(goal)) {
        stop();
        this.follows.delete(goal);
      }
    for (const goal of next)
      if (!this.follows.has(key(goal))) this.follow(key(goal), goal);
    return this.refresh();
  }

  /** Read the scope again; equivalent results neither repaint nor signal. */
  async refresh(): Promise<void> {
    this.reading?.abort();
    const epoch = ++this.reads;
    if (!this.goals.length) {
      this.reading = undefined;
      const had = this.list !== undefined || this.readFailure !== undefined;
      this.list = undefined;
      this.readFailure = undefined;
      this.publish();
      if (had) this.host.changed();
      return;
    }
    const controller = new AbortController();
    this.reading = controller;
    const before = JSON.stringify([this.list, this.readFailure]);
    try {
      const list = await this.host.source.list(this.goals, controller.signal);
      if (epoch !== this.reads || this.lifetime.signal.aborted) return;
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
    if (JSON.stringify([this.list, this.readFailure]) !== before)
      this.host.changed();
  }

  /** Follow one goal from the store's current head; a reconnect replays from the cursor. */
  private follow(key: string, goal: GoalRef): void {
    const tasks = this.host.tasks;
    if (!tasks) return;
    let stop: (() => void) | undefined;
    let stopped = false;
    this.follows.set(key, () => {
      stopped = true;
      stop?.();
    });
    void tasks.head(goal.project).then(
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
      () => this.follows.delete(key),
    );
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

  /** Only the responsible coordinator surfaces native attention, once per change. */
  private publish(): void {
    const requests = this.responsible()
      .flatMap(goal => goal.requests)
      .filter(request => request.state === 'open')
      .sort((a, b) => a.created_at - b.created_at)
      .map(request => ({id: request.id, title: request.title}));
    const key = JSON.stringify(requests);
    if (key === this.signalled) return;
    this.signalled = key;
    this.host.signal({requests});
  }

  private deliverAnswers(): void {
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
    this.host.deliver(() => answerNotice(answered));
  }

  /**
   * Fresh facts for each goal's context section: requests and mandates,
   * decisions, and the request a request-linked mandate rests on. Requests
   * are immutable once answered, so resolved sources are cached.
   */
  async facts(): Promise<Map<string, AttentionFacts>> {
    await this.refresh();
    const result = new Map<string, AttentionFacts>();
    const goals = this.goals;
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
    for (const stop of this.follows.values()) stop();
    this.follows.clear();
    this.lifetime.abort();
    this.reading?.abort();
    this.reads++;
  }
}
