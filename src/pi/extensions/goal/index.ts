import {Type} from 'typebox';
import {MouseRegion} from '@earendil-works/pi-tui';
import type {
  ExtensionAPI,
  ExtensionContext,
  Theme,
} from '@earendil-works/pi-coding-agent';
import {
  WORKER_ACTIVITY_EVENT,
  workerActivitySnapshot,
  type WorkerActivitySnapshot,
} from '../../src/ui/worker-activity.ts';
import {PriorityRow, labelText, sourceText} from '@joshrwolf/pi-kit/ui';
import {
  inspection,
  compactSummary,
  type Focus,
  type GoalView,
} from './present.ts';
import {summaryRow} from './summary.ts';
import {openRexGoals} from '../rex/index.ts';
import {GOAL_SECTION_PREFIX, goalSections, orientation} from './context.ts';
import {bindAgent, endAgent, type Agent} from '../../src/host/agent.ts';
import {watchChanges} from '../../src/host/changes.ts';
import {
  dotsSource,
  dotsEvidenceSource,
  shareChanges,
  type EvidenceSource,
  type GoalSource,
  type GoalList,
  type GoalRef,
} from './source.ts';

import {
  EVIDENCE_HELP,
  evidenceArgs,
  evidenceRecordText,
  inspectEvidence,
} from './evidence.ts';

export const FOCUS_ENTRY = 'dots.goal.focus';
const WIDGET = 'dots.goal';
export const DISMISS_ENTRY = 'dots.goal.dismiss';
const OFFERS_WIDGET = 'dots.goal.offers';
const HELP =
  "/goal [new IDEA | follow [ID [PROJECT]] | show [ID] | evidence TASK [EVENT|current] [PROJECT] | list | followed | offers | dismiss ID | refresh | unfollow ID | clear]. PROJECT is another project's exact key (the project field of dots task --json output); without it, follow uses this session's project. focus aliases follow; unfocus without an ID clears all follows. Goals this session coordinates stay in view without a follow until paused, released, handed off or closed; unfollowing never does any of those. No shared task state is changed.";

export function restoreGoals(
  entries: readonly {type: string; customType?: string; data?: unknown}[],
): {focus?: Focus; followed: Focus[]} {
  for (const entry of [...entries].reverse()) {
    if (entry.type !== 'custom' || entry.customType !== FOCUS_ENTRY) continue;
    const value = entry.data as
      {version?: unknown; focus?: unknown; followed?: unknown} | undefined;
    if (value?.version !== 2)
      throw new Error('Unsupported saved goal focus; use /goal clear.');
    const decode = (raw: unknown): Focus => {
      const candidate = raw as Partial<Focus> | undefined;
      if (
        !candidate ||
        typeof candidate.id !== 'string' ||
        typeof candidate.project !== 'string' ||
        !/^t-[a-f0-9]+$/.test(candidate.id) ||
        !candidate.project
      )
        throw new Error('Invalid saved goal focus; use /goal clear.');
      return {id: candidate.id, project: candidate.project};
    };
    const focus = value.focus === null ? undefined : decode(value.focus);
    if (!Array.isArray(value.followed))
      throw new Error('Invalid saved followed goals; use /goal clear.');
    const followed = value.followed.map(decode);
    if (
      new Set(followed.map(goal => goal.id)).size !== followed.length ||
      (focus
        ? !followed.some(
            goal => goal.id === focus.id && goal.project === focus.project,
          )
        : followed.length > 0)
    )
      throw new Error('Invalid saved goal selection; use /goal clear.');
    return {...(focus ? {focus} : {}), followed};
  }
  return {followed: []};
}

export function restoreFocus(
  entries: Parameters<typeof restoreGoals>[0],
): Focus | undefined {
  return restoreGoals(entries).focus;
}

/** Shared change notification and native view navigation. */
export interface GoalOptions {
  /**
   * The changes marker to watch for other machines' task trees, or undefined
   * when this machine does not share them.
   */
  readonly changes?: (
    cwd: string,
    signal: AbortSignal,
  ) => Promise<string | undefined>;
  readonly openView?: (
    goal: {project: string; id: string} | undefined,
  ) => Promise<void>;
}

export default function goalExtension(
  pi: ExtensionAPI,
  source: GoalSource = dotsSource,
  evidenceSource: EvidenceSource = dotsEvidenceSource,
  options: GoalOptions = {},
): void {
  const changesMarker = options.changes ?? shareChanges;
  /** Stops watching imports for the current session. */
  let stopImports: (() => void) | undefined;
  /**
   * Goals the last successful coordination read found this runtime
   * coordinating; a failed read keeps them shown, with its failure.
   */
  let duties: readonly GoalRef[] = [];
  let dutyFailure: string | undefined;
  let dutyReads = 0;
  let dutyRead: AbortController | undefined;
  let focus: Focus | undefined;
  let model: GoalView | undefined;
  /** This session's goals: its follows plus the goals it coordinates. */
  const views = new Map<string, GoalView>();
  /** Explicit follows, all in views: the only membership a branch records. */
  const follows = new Set<string>();
  const dismissed = new Set<string>();
  let offers: GoalList | undefined;
  let discovery: AbortController | undefined;
  const offerKey = (project: string, id: string) =>
    JSON.stringify([project, id]);
  const candidates = () =>
    offers?.goals.filter(
      ({root}) =>
        !dismissed.has(offerKey(offers!.project, root.id)) &&
        views.get(root.id)?.focus.project !== offers!.project,
    ) ?? [];
  const offerLines = (limit = 3) => {
    const goals = candidates();
    return goals
      .slice(0, limit)
      .map(
        ({root}) =>
          `Working on ${labelText(root.title)} · Follow: /goal follow ${root.id} · Dismiss: /goal dismiss ${root.id}`,
      )
      .concat(goals.length > limit ? ['More claimed goals: /goal offers'] : []);
  };
  const drawOffers = (ctx: ExtensionContext) => {
    if (active && ctx.mode === 'tui' && ctx.hasUI) {
      const lines = offerLines();
      ctx.ui.setWidget(OFFERS_WIDGET, lines.length ? lines : undefined, {
        placement: 'belowEditor',
      });
    }
  };
  const dismiss = (project: string, id: string) => {
    const key = offerKey(project, id);
    if (dismissed.has(key)) return;
    dismissed.add(key);
    pi.appendEntry(DISMISS_ENTRY, {project, id});
  };
  const discover = async (ctx: ExtensionContext) => {
    // A worker does not take on goals; offering them is its owner's concern.
    if (!active || !agent || process.env.DOTS_WORKER_ID) return false;
    const claimedBy = agent.owner;
    discovery?.abort();
    const controller = new AbortController();
    discovery = controller;
    const epoch = generation;
    try {
      const result = await source.list(ctx.cwd, controller.signal, claimedBy);
      if (!active || generation !== epoch || controller.signal.aborted)
        return false;
      offers = result;
      drawOffers(ctx);
    } catch (error) {
      if (!active || generation !== epoch || controller.signal.aborted)
        return false;
      offers = undefined;
      drawOffers(ctx);
      // A failed query is not evidence that no goals are claimed.
      if (ctx.hasUI)
        ctx.ui.setStatus(
          OFFERS_WIDGET,
          'Goal suggestions unavailable · /goal offers',
        );
      return String(error);
    } finally {
      if (discovery === controller) discovery = undefined;
    }
    if (ctx.hasUI) ctx.ui.setStatus(OFFERS_WIDGET, undefined);
    return undefined;
  };
  const followedViews = () => [...follows].map(id => views.get(id)!);
  /** Record the follows; a coordinated goal's selection is not recorded. */
  const persist = () => {
    const followed = followedViews().map(view => view.focus);
    pi.appendEntry(FOCUS_ENTRY, {
      version: 2,
      focus: (focus && follows.has(focus.id) ? focus : followed[0]) ?? null,
      followed,
    });
  };
  /**
   * Bring goals this session coordinates into view and drop those it no
   * longer coordinates, as the store last said; follows stay. Returns whether
   * a goal joined and needs reading.
   */
  const reconcile = (ctx: ExtensionContext): boolean => {
    if (!active) return false;
    const coordinated = new Set(duties.map(goal => goal.id));
    let joined = false;
    let left = false;
    for (const goal of duties)
      if (!views.has(goal.id)) {
        views.set(goal.id, {focus: {id: goal.id, project: goal.project}});
        joined = true;
      }
    for (const id of views.keys())
      if (!follows.has(id) && !coordinated.has(id)) {
        views.delete(id);
        left = true;
      }
    if (!joined && !left) return false;
    if (!focus || !views.has(focus.id))
      focus = views.values().next().value?.focus;
    model = focus ? views.get(focus.id) : undefined;
    draw(ctx);
    return joined;
  };
  let generation = 0;
  let active = false;
  let request:
    | {
        controller: AbortController;
        pending: boolean;
        selected: boolean;
        /** The goals this read observes. */
        targets: readonly string[];
        failure?: Error;
        result: Promise<GoalView | undefined>;
      }
    | undefined;
  let dialog: AbortController | undefined;
  let mounted = false;
  /** This runtime's actor, bound once at session start; reload keeps it. */
  let agent: Agent | undefined;
  let workerActivity: WorkerActivitySnapshot | undefined;
  let stopWorkerActivity: (() => void) | undefined;
  let summary: {row: PriorityRow; theme: Theme; redraw: () => void} | undefined;
  const notify = (
    ctx: ExtensionContext,
    message: string,
    type: 'info' | 'error' = 'info',
    recovery = 'dots task goal list --json for complete shared records',
  ) => {
    if (ctx.hasUI)
      ctx.ui.notify(
        sourceText(message).slice(0, 2000) +
          (message.length > 2000
            ? `\n[Display limited; use ${recovery}.]`
            : ''),
        type,
      );
    else
      pi.sendMessage(
        {customType: 'dots.goal.notice', content: message, display: true},
        {triggerTurn: false},
      );
  };
  /** Repaint the summary line and an open panel from current observations. */
  const paint = () => {
    if (summary) {
      summary.row.update(
        summaryRow(
          [...views.values()],
          focus?.id,
          workerActivity,
          agent?.owner,
        ),
        summary.theme,
      );
      summary.redraw();
    }
  };
  const select = (ctx: ExtensionContext, chosen: GoalView) => {
    focus = chosen.focus;
    model = chosen;
    // The branch records follows; a coordinated goal's selection is transient.
    if (follows.has(chosen.focus.id)) persist();
    draw(ctx);
  };
  /** Open the shared Rex surface; the local scope and editor draft stay unchanged. */
  const openView = (ctx: ExtensionContext) => {
    if (!active || ctx.mode !== 'tui' || !ctx.hasUI) return;
    const epoch = generation;
    const id = model?.focus.id ?? focus?.id;
    const project =
      model?.snapshot?.project ?? model?.focus.project ?? focus?.project;
    if (id && !project) {
      notify(
        ctx,
        'The goal project is unavailable. Use /goal refresh before opening it.',
        'error',
      );
      return;
    }
    const target = id && project ? {id, project} : undefined;
    const open =
      options.openView ?? (target => openRexGoals(pi, target, process.env));
    open(target).catch((error: unknown) => {
      if (active && epoch === generation) notify(ctx, String(error), 'error');
    });
  };
  const draw = (ctx: ExtensionContext) => {
    if (!active || ctx.mode !== 'tui' || !ctx.hasUI) return;
    drawOffers(ctx);
    if (!model) {
      if (mounted) ctx.ui.setWidget(WIDGET, undefined);
      summary = undefined;
      mounted = false;
      return;
    }
    if (!mounted) {
      mounted = true;
      const epoch = generation;
      ctx.ui.setWidget(
        WIDGET,
        (tui, theme) => {
          const owned = {
            row: new PriorityRow(),
            theme,
            redraw: () => tui.requestRender(),
          };
          summary = owned;
          paint();
          const region = new MouseRegion(
            {
              render: width =>
                active && epoch === generation ? owned.row.render(width) : [],
              invalidate: () => owned.row.invalidate(),
            },
            event => {
              if (
                !active ||
                epoch !== generation ||
                summary !== owned ||
                dialog ||
                event.button !== 'left' ||
                event.shift ||
                event.alt ||
                event.ctrl
              )
                return undefined;
              // Pi synthesizes a click after an accepted press, only when it was not a drag.
              if (event.type === 'press') return {handled: true, render: false};
              if (event.type !== 'click') return undefined;
              openView(ctx);
              return {handled: true, render: false};
            },
          );
          return Object.assign(region, {
            dispose: () => {
              if (summary === owned) {
                summary = undefined;
                mounted = false;
              }
            },
          });
        },
        {placement: 'belowEditor'},
      );
    }
    paint();
  };
  const invalidate = (ctx?: ExtensionContext) => {
    generation++;
    discovery?.abort();
    discovery = undefined;
    request?.controller.abort();
    request = undefined;
    dialog?.abort();
    dialog = undefined;
    if (ctx && mounted && ctx.mode === 'tui' && ctx.hasUI)
      ctx.ui.setWidget(WIDGET, undefined);
    mounted = false;
    summary = undefined;
  };

  // Ordinary observers share an in-flight read, including an explicit focus
  // change. Only explicit selection, or a goal joining the view after an
  // ordinary read began, may supersede it. Observers follow that
  // replacement and consume its result, never the previous global model.
  // A selection rejects with its own failure; undefined means cancelled or superseded.
  // Each goal is read from its recorded project, not the session cwd.
  const refresh = async (
    ctx: ExtensionContext,
    selected?: {id: string; project?: string | undefined},
    signal?: AbortSignal,
  ): Promise<GoalView | undefined> => {
    const epoch = generation;
    if (!active || signal?.aborted) return undefined;
    const stale = (read: NonNullable<typeof request>) =>
      !read.selected &&
      [...views.keys()].some(id => !read.targets.includes(id));
    if (selected || !request?.pending || stale(request)) {
      if (!selected && !focus) return undefined;
      request?.controller.abort();
      const read: NonNullable<typeof request> = {
        controller: new AbortController(),
        pending: true,
        selected: !!selected,
        targets: selected ? [selected.id] : [...views.keys()],
        result: Promise.resolve<GoalView | undefined>(undefined),
      };
      request = read;
      const abort = () => read.controller.abort();
      signal?.addEventListener('abort', abort, {once: true});
      const current = () =>
        active &&
        generation === epoch &&
        request === read &&
        !read.controller.signal.aborted;
      read.result = (async () => {
        try {
          const observations = await Promise.all(
            read.targets.map(async (target): Promise<GoalView> => {
              const saved = views.get(target)?.focus;
              const project = selected?.project ?? saved?.project;
              try {
                const snapshot = await source.show(
                  ctx.cwd,
                  target,
                  read.controller.signal,
                  false,
                  project,
                );
                if (snapshot.overview.goal.root.id !== target)
                  throw new Error('dots returned a different goal');
                if (project && snapshot.project !== project)
                  throw new Error('dots returned a different project');
                if (!current()) throw new Error('Goal read cancelled');
                const brief = snapshot.overview.goal.briefContent;
                return {
                  focus: {id: target, project: snapshot.project},
                  snapshot,
                  brief,
                };
              } catch (error) {
                if (selected) throw error;
                return {focus: saved!, error: String(error)};
              }
            }),
          );
          if (!current()) return undefined;
          // A goal that left the view meanwhile stays gone.
          for (const observation of observations)
            if (selected || views.has(observation.focus.id))
              views.set(observation.focus.id, observation);
          if (selected) {
            focus = observations[0]!.focus;
            follows.add(focus.id);
            persist();
          }
          model = focus ? views.get(focus.id) : undefined;
          draw(ctx);
          return model;
        } catch (error) {
          if (!current()) return undefined;
          if (selected)
            read.failure =
              error instanceof Error
                ? error
                : new Error(String(error), {cause: error});
          // A rejected addition must not overwrite another goal's observation.
          // Pre-run observers refresh the goals after selection settles.
          if (focus) return {focus, error: String(error)};
          return undefined;
        } finally {
          read.pending = false;
          signal?.removeEventListener('abort', abort);
        }
      })();
    }
    let observed = request;
    while (observed) {
      const result = await observed.result;
      if (!active || generation !== epoch) return undefined;
      if (request === observed) {
        if (selected && observed.failure !== undefined) throw observed.failure;
        return result;
      }
      if (selected) return undefined;
      observed = request;
    }
    return undefined;
  };

  /**
   * Read the goals this runtime coordinates, fresh from the store's root
   * claims; a later read supersedes an earlier one. Managed workers never
   * coordinate. A failed read keeps the last goals: unknown is not none.
   */
  const readDuties = async (): Promise<void> => {
    dutyRead?.abort();
    const actor = agent;
    if (!active || !actor || process.env.DOTS_WORKER_ID) return;
    const epoch = ++dutyReads;
    const controller = new AbortController();
    dutyRead = controller;
    try {
      const goals = await source.coordinating(actor, controller.signal);
      if (epoch !== dutyReads) return;
      duties = goals;
      dutyFailure = undefined;
    } catch (error) {
      if (epoch !== dutyReads) return;
      dutyFailure = String(error);
    } finally {
      if (dutyRead === controller) dutyRead = undefined;
    }
  };
  /** Reread coordination, then read any goal that joined the view. */
  const coordinate = (ctx: ExtensionContext) => {
    readDuties()
      .then(() => (reconcile(ctx) ? refresh(ctx) : undefined))
      .catch((error: unknown) => {
        if (active) notify(ctx, String(error), 'error');
      });
  };

  /**
   * Imported trees change only when the sync agent imports another machine's
   * writes; it then touches the substrate's changes marker. Reread the goals
   * in view and coordination when it does, as runs and explicit reads would.
   */
  const watchImports = (ctx: ExtensionContext) => {
    const lifetime = new AbortController();
    let watcher: {readonly close: () => void} | undefined;
    changesMarker(ctx.cwd, lifetime.signal).then(
      marker => {
        if (!marker || lifetime.signal.aborted) return;
        watcher = watchChanges(marker, () => {
          if (!active || lifetime.signal.aborted) return;
          coordinate(ctx);
          if (views.size)
            refresh(ctx).catch((error: unknown) =>
              notify(ctx, String(error), 'error'),
            );
        });
      },
      // Without sharing status, goals refresh on runs and explicit reads.
      () => {},
    );
    return () => {
      lifetime.abort();
      watcher?.close();
    };
  };

  const restore = async (ctx: ExtensionContext) => {
    invalidate(ctx);
    workerActivity = undefined;
    active = true;
    focus = undefined;
    model = undefined;
    views.clear();
    follows.clear();
    offers = undefined;
    dismissed.clear();
    for (const entry of ctx.sessionManager.getBranch()) {
      if (entry.type !== 'custom' || entry.customType !== DISMISS_ENTRY)
        continue;
      const data = entry.data as Partial<Focus> | undefined;
      if (typeof data?.project === 'string' && typeof data.id === 'string')
        dismissed.add(offerKey(data.project, data.id));
    }
    drawOffers(ctx);
    try {
      const saved = restoreGoals(ctx.sessionManager.getBranch());
      focus = saved.focus;
      for (const item of saved.followed) {
        views.set(item.id, {focus: item});
        follows.add(item.id);
      }
    } catch (error) {
      notify(ctx, String(error), 'error');
    }
    reconcile(ctx);
    coordinate(ctx);
    const epoch = generation;
    if (focus) {
      model = views.get(focus.id);
      draw(ctx);
      await refresh(ctx);
    }
    if (active && epoch === generation && ctx.mode === 'tui' && ctx.hasUI)
      await discover(ctx);
  };
  pi.on('session_start', (event, ctx) => {
    agent = bindAgent(ctx.sessionManager.getSessionId(), event.reason);
    dutyRead?.abort();
    dutyReads++;
    duties = [];
    dutyFailure = undefined;
    stopImports?.();
    stopImports = watchImports(ctx);
    stopWorkerActivity?.();
    workerActivity = undefined;
    stopWorkerActivity = pi.events.on(WORKER_ACTIVITY_EVENT, data => {
      if (!active || !model) return;
      workerActivity = workerActivitySnapshot(data);
      paint();
    });
    return restore(ctx);
  });
  pi.on('session_tree', (_event, ctx) => restore(ctx));
  pi.on('session_shutdown', async (event, ctx) => {
    // A claim names the runtime working on a task; an ended runtime is not.
    // Reload keeps the runtime, and workers never claim. Release acts only
    // as this runtime's own actor, never a later one's.
    const ending = agent;
    if (ending && event.reason !== 'reload') {
      if (!process.env.DOTS_WORKER_ID)
        await source.release(ending).catch((error: unknown) => {
          if (ctx.hasUI)
            ctx.ui.notify(`Claims not released: ${String(error)}`, 'warning');
        });
      endAgent(ending);
      agent = undefined;
    }
    if (ctx.mode === 'tui' && ctx.hasUI)
      ctx.ui.setWidget(OFFERS_WIDGET, undefined);
    if (ctx.hasUI) ctx.ui.setStatus(OFFERS_WIDGET, undefined);
    active = false;
    stopImports?.();
    stopImports = undefined;
    dutyRead?.abort();
    dutyReads++;
    duties = [];
    dutyFailure = undefined;
    stopWorkerActivity?.();
    stopWorkerActivity = undefined;
    workerActivity = undefined;
    invalidate();
    focus = undefined;
    model = undefined;
    views.clear();
    follows.clear();
    offers = undefined;
  });
  pi.on('before_agent_start', async (event, ctx) => {
    const sections = event.systemPromptOptions.sections;
    for (const key of Object.keys(sections))
      if (key.startsWith(GOAL_SECTION_PREFIX)) delete sections[key];
    const startingEpoch = generation;
    if (ctx.mode === 'tui' && ctx.hasUI) await discover(ctx);
    if (!active || startingEpoch !== generation) return;
    // Membership changes invalidate in-flight observations. Retry the remaining
    // set; an unfollow must neither resurrect a goal nor omit the other goals.
    const observe = async () => {
      while (active && (focus || request?.pending)) {
        const epoch = generation;
        await refresh(ctx);
        if (!active) return;
        if (generation !== epoch) {
          if (views.size || request?.pending) continue;
          break;
        }
        if (!views.size) break;
        if (request?.selected) continue; // Explicit follow reads only its target.
        break;
      }
    };
    await observe();
    if (!active) return;
    // Coordination is read fresh; a goal that joined, here or in a read it
    // started, is read before the sections are built.
    await readDuties();
    if (!active) return;
    reconcile(ctx);
    if ([...views.values()].some(view => !view.snapshot && !view.error))
      await observe();
    if (!active) return;
    if (dutyFailure)
      sections[`${GOAL_SECTION_PREFIX}coordinating`] =
        `Goals this session coordinates could not be checked: ${dutyFailure}. The goals in context may be out of date; check them with dots task goal coordinating before dispatching work.`;
    for (const view of views.values())
      Object.assign(sections, goalSections(view, agent?.owner));
  });
  pi.on('agent_end', async (_event, ctx) => {
    const epoch = generation;
    coordinate(ctx);
    await refresh(ctx);
    if (active && epoch === generation && ctx.mode === 'tui' && ctx.hasUI)
      await discover(ctx);
  });

  /** TUI: open centralized Goals. Headless: one text inspection message. */
  const show = async (ctx: ExtensionContext) => {
    const epoch = generation;
    if (ctx.mode === 'tui' && ctx.hasUI) {
      openView(ctx);
      if (model) await refresh(ctx);
      return;
    }
    const observed = await refresh(ctx);
    if (!active || generation !== epoch) return;
    if (!observed)
      notify(ctx, 'No focused goal. Use /goal follow ID or /goal list.');
    else
      pi.sendMessage(
        {
          customType: 'dots.goal.inspection',
          content: inspection(observed),
          display: true,
        },
        {triggerTurn: false},
      );
  };
  pi.registerTool({
    name: 'goal_focus',
    label: 'Focus goal',
    description:
      'Follow an existing goal in this session, additively, and select its details. Goals this session coordinates are already in its view and context; follow any other goal to watch it read-only, or to select a coordinated goal right after coordinating it. Every goal in view gets fresh brief/task context before runs. Other goals stay in view; no tasks, claims, or execution state change. Use goal_unfocus to remove a follow. Create shared goals with dots task goal create TITLE --brief PATH --json.',
    parameters: Type.Object({
      id: Type.String({pattern: '^t-[a-f0-9]+$', maxLength: 64}),
      project: Type.Optional(
        Type.String({
          minLength: 1,
          maxLength: 4096,
          description:
            "Exact project key of a goal in another project, from the project field of dots task --json output; omit for this session's project or an already-followed goal",
        }),
      ),
    }),
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    },
    async execute(_callId, {id, project}, signal, _update, ctx) {
      if (signal?.aborted) throw new Error('Goal focus cancelled');
      if (dialog) throw new Error('A goal dialog is already open');
      const controller = new AbortController();
      dialog = controller;
      try {
        const observed = await refresh(ctx, {id, project}, signal);
        if (!observed || observed.focus.id !== id)
          throw new Error('Goal focus was interrupted');
        return {
          content: [{type: 'text', text: orientation(observed, agent?.owner)}],
          details: {id: observed.focus.id, project: observed.focus.project},
        };
      } finally {
        if (dialog === controller) dialog = undefined;
      }
    },
  });

  const unfollow = (ctx: ExtensionContext, id?: string) => {
    invalidate(ctx);
    const coordinated = new Set(duties.map(goal => goal.id));
    const leaving = id ? [id].filter(goal => follows.has(goal)) : [...follows];
    for (const goal of leaving) {
      dismiss(views.get(goal)!.focus.project, goal);
      follows.delete(goal);
      if (!coordinated.has(goal)) views.delete(goal);
    }
    if (!focus || !views.has(focus.id))
      focus = views.values().next().value?.focus;
    model = focus ? views.get(focus.id) : undefined;
    persist();
    draw(ctx);
    const unchanged = 'Shared goals, tasks and claims are unchanged.';
    const stays =
      'Unfollowing does not pause, release or hand off a goal; a goal this session coordinates stays in its view and context until one of those happens or it closes.';
    if (!id) {
      const kept = leaving.filter(goal => coordinated.has(goal));
      return kept.length
        ? `All follows cleared. ${unchanged} Still coordinating, so still in view: ${kept.join(', ')}. ${stays}`
        : `All follows cleared. ${unchanged}`;
    }
    if (!leaving.length)
      return coordinated.has(id)
        ? `${id} is not followed; this session coordinates it. ${stays}`
        : `${id} is not followed.`;
    return coordinated.has(id)
      ? `Unfollowed ${id}, but this session coordinates it, so it stays in view. ${stays} ${unchanged}`
      : `Unfollowed ${id}: its brief, tasks and popup leave this session. ${unchanged}`;
  };
  pi.registerTool({
    name: 'goal_unfocus',
    label: 'Unfollow goal',
    description:
      "Remove one follow from this session, or clear all follows on request. A goal leaves this session's view and context only if the session does not coordinate it: unfollowing is not pausing, releasing or handing off, and a coordinated goal stays until one of those happens or it closes. Does not change shared goals, tasks, claims, workers or other sessions, or remove prior conversation. Specify id or all:true.",
    parameters: Type.Object({
      id: Type.Optional(Type.String({pattern: '^t-[a-f0-9]+$', maxLength: 64})),
      all: Type.Optional(Type.Boolean()),
    }),
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    },
    async execute(_callId, {id, all}, signal, _update, ctx) {
      if (signal?.aborted) throw new Error('Goal unfocus cancelled');
      if ((!id && all !== true) || (id && all === true))
        throw new Error('Specify id or all:true, not both.');
      return {
        content: [{type: 'text', text: unfollow(ctx, id)}],
        details: {followed: [...follows]},
      };
    },
  });
  pi.registerTool({
    name: 'goal_list',
    label: 'List session goals',
    description:
      "List this session's goals, the ones it follows and the ones it coordinates, and the selected details view. Read-only session state; use dots task goal list --json for all shared project goals. Selection is not execution state.",
    parameters: Type.Object({}),
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    },
    async execute() {
      const coordinated = new Set(duties.map(goal => goal.id));
      const goals = [...views.values()].map(view => ({
        ...view.focus,
        title: view.snapshot?.overview.goal.root.title,
        selected: view.focus.id === focus?.id,
        followed: follows.has(view.focus.id),
        coordinating: coordinated.has(view.focus.id),
      }));
      return {
        content: [{type: 'text', text: JSON.stringify({goals})}],
        details: {goals},
      };
    },
  });

  pi.registerCommand('goal', {
    description:
      'Follow multiple goals; without arguments, open centralized Rex Goals: [new IDEA|follow [ID [PROJECT]]|show [ID]|evidence TASK [EVENT|current] [PROJECT]|list|followed|offers|dismiss ID|refresh|unfollow ID|clear]',
    handler: async (args, ctx) => {
      const words = args.trim().split(/\s+/).filter(Boolean);
      const action = words[0] ?? (focus ? 'show' : 'focus');
      if (action === 'evidence') {
        if (dialog) {
          notify(ctx, 'A goal dialog is already open.');
          return;
        }
        const controller = new AbortController();
        dialog = controller;
        const epoch = generation;
        const current = () =>
          active && epoch === generation && !controller.signal.aborted;
        try {
          const {id, seq, project} = evidenceArgs(words.slice(1));
          const record = await evidenceSource.record(
            ctx.cwd,
            id,
            controller.signal,
            seq,
            project,
          );
          if (!current()) return;
          if (ctx.mode === 'tui' && ctx.hasUI)
            await inspectEvidence(
              ctx,
              record,
              evidenceSource,
              controller.signal,
            );
          else
            pi.sendMessage(
              {
                customType: 'dots.goal.evidence',
                content: evidenceRecordText(record),
                display: true,
              },
              {triggerTurn: false},
            );
        } catch (error) {
          if (current())
            notify(ctx, `${String(error)}\n${EVIDENCE_HELP}`, 'error');
        } finally {
          if (dialog === controller) dialog = undefined;
        }
        return;
      }
      if (action === 'new') {
        const idea = args.trim().slice(3).trim();
        pi.sendUserMessage(
          [
            'Help me shape a goal' +
              (idea ? `: ${idea}` : '. Ask what I want to accomplish.'),
            'Discuss only what is unclear. When there is enough direction, capture a concise Markdown brief (the agreed outcome and execution mandate) in a suitable document, then use dots task goal create TITLE --brief PATH --json to create its root and association together. If an appropriate root already exists, attach the brief instead. Add useful initial slices under that root and call goal_focus with its id so I can see it here.',
            'This request is to establish shared context and tracking, not to start implementation. Do not invent an execution policy or a fixed pipeline. Use the shared goals skill and worktrees skill when isolation is useful; Git and provider state are evidence, task status is coordination.',
          ].join('\n\n'),
          {deliverAs: 'followUp'},
        );
        return;
      }
      if (
        action !== 'focus' &&
        action !== 'follow' &&
        words.length >
          (['show', 'unfocus', 'unfollow', 'dismiss'].includes(action) ? 2 : 1)
      ) {
        notify(ctx, HELP, 'error');
        return;
      }
      if (action === 'offers' || action === 'dismiss') {
        const epoch = generation;
        const error = await discover(ctx);
        if (!active || epoch !== generation || error === false) return;
        if (error) {
          notify(ctx, error, 'error');
          return;
        }
        if (action === 'dismiss') {
          const id = words[1];
          if (!id || !offers?.goals.some(({root}) => root.id === id)) {
            notify(
              ctx,
              'Use /goal dismiss ID for a currently claimed goal.',
              'error',
            );
            return;
          }
          dismiss(offers.project, id);
          drawOffers(ctx);
        } else
          notify(
            ctx,
            offerLines(Infinity).join('\n') ||
              'No unfollowed goals claimed by this session.',
          );
        return;
      }
      if (action === 'unfocus' || action === 'unfollow' || action === 'clear') {
        if (action === 'unfollow' && !words[1]) {
          notify(ctx, 'Use /goal unfollow ID or /goal clear.', 'error');
          return;
        }
        if (words[1] && !/^t-[a-f0-9]+$/.test(words[1])) {
          notify(ctx, HELP, 'error');
          return;
        }
        notify(ctx, unfollow(ctx, words[1]));
        return;
      }
      if (action === 'followed') {
        notify(
          ctx,
          views.size
            ? [...views.values()]
                .map(
                  view =>
                    `${view.focus.id === focus?.id ? '> ' : '  '}${view.focus.id} ${view.snapshot?.overview.goal.root.title ?? ''} · ${compactSummary(view)}${follows.has(view.focus.id) ? '' : ' · coordinating'}`,
                )
                .join('\n')
            : 'No goals followed or coordinated.',
          'info',
          'goal_list for complete session goals',
        );
        return;
      }
      if (action === 'show') {
        if (words[1]) {
          const chosen = views.get(words[1]);
          if (!chosen) {
            notify(
              ctx,
              'Goal is not followed or coordinated here. Use /goal follow ID first.',
              'error',
            );
            return;
          }
          select(ctx, chosen);
        }
        await show(ctx);
        return;
      }
      if (dialog) {
        notify(ctx, 'A goal dialog is already open.');
        return;
      }
      if (action === 'refresh') {
        const epoch = generation;
        await readDuties();
        if (!active || generation !== epoch) return;
        reconcile(ctx);
        await refresh(ctx);
        if (!active || generation !== epoch) return;
        while (request?.selected && views.size) {
          await refresh(ctx);
          if (!active || generation !== epoch) return;
        }
        const failing = [...views.values()].filter(
          view => view.error || view.brief?.warning,
        );
        const status = views.size
          ? `${views.size} goal(s) refreshed (local records only).`
          : 'No goals followed or coordinated.';
        notify(
          ctx,
          failing.length
            ? `${status} ${failing.length} need attention.\n${failing.map(view => `${view.focus.id}: ${view.error ?? view.brief?.warning}`).join('\n')}`
            : status,
          failing.some(view => view.error) ? 'error' : 'info',
        );
        return;
      }
      if (action !== 'focus' && action !== 'follow' && action !== 'list') {
        notify(ctx, HELP, 'error');
        return;
      }
      const controller = new AbortController();
      dialog = controller;
      const epoch = generation;
      const current = () =>
        active && generation === epoch && !controller.signal.aborted;
      try {
        let id = words[1];
        if (!id) {
          const list = await source.list(ctx.cwd, controller.signal);
          if (!current()) return;
          if (action === 'list' || !ctx.hasUI) {
            notify(
              ctx,
              list.goals.length
                ? list.goals
                    .map(
                      g =>
                        `${g.root.id} [${g.root.status}]${follows.has(g.root.id) ? ' [followed]' : views.has(g.root.id) ? ' [coordinating]' : ''} ${g.root.title}`,
                    )
                    .join('\n') + '\nUse /goal focus ID.'
                : 'No open goals. Attach a brief using dots task goal attach ID --brief PATH.',
            );
            return;
          }
          if (!list.goals.length) {
            notify(
              ctx,
              'No open goals. Attach a brief using dots task goal attach ID --brief PATH.',
            );
            return;
          }
          const choices = list.goals.map(
            g => `${g.root.id} · ${labelText(g.root.title)}`,
          );
          const selected = await ctx.ui.select(
            'Follow a goal (adds context; does not start work)',
            choices,
            {signal: controller.signal},
          );
          if (!current() || selected === undefined) return;
          id = list.goals[choices.indexOf(selected)]?.root.id;
        }
        if (!id || !/^t-[a-f0-9]+$/.test(id)) {
          notify(ctx, 'Expected a task root ID, such as t-123abc.', 'error');
          return;
        }
        // A project key is the rest of the arguments; keys are paths and may contain spaces.
        const project = args
          .trim()
          .match(/^\S+\s+\S+\s+(.+)$/s)?.[1]
          ?.trim();
        const observed = await refresh(ctx, {id, project});
        if (observed && observed.focus.id === id && current())
          notify(
            ctx,
            `Focused ${id}; following ${follows.size} goal(s). Other goals are unchanged. No work started. Use /goal to inspect.`,
          );
      } catch (error) {
        if (current()) notify(ctx, String(error), 'error');
      } finally {
        if (dialog === controller) dialog = undefined;
      }
    },
  });
}
