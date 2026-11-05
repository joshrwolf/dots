import {Type} from 'typebox';
import {MouseRegion, truncateToWidth} from '@earendil-works/pi-tui';
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
import {GoalPanel, PANEL_KEY, panelOverlay, summaryRow} from './panel.ts';
import {
  GOAL_SECTION_PREFIX,
  goalSectionKey,
  goalSections,
  orientation,
} from './context.ts';
import {bindAgent, endAgent, type Agent} from '../../src/host/agent.ts';
import {ATTENTION_EVENT} from '../../src/host/attention.ts';
import {TurnMessages} from '../../src/host/delivery.ts';
import {Tasks} from '../../src/host/tasks.ts';
import {AttentionController} from './attention/controller.ts';
import {attentionContext} from './attention/context.ts';
import {indicator} from './attention/model.ts';
import {
  AttentionView,
  GoalPopup,
  popupBodyHeight,
  type PopupView,
} from './attention/panel.ts';
import {
  HumanInputs,
  RECORD_CHAT_DESCRIPTION,
  RecordChatParameters,
  recordChat,
} from './attention/chat.ts';
import {
  dotsAttention,
  failure as attentionFailure,
  type AttentionSource,
  type GoalRef,
} from './attention/source.ts';
import {
  dotsSource,
  dotsEvidenceSource,
  type EvidenceSource,
  type GoalSource,
  type GoalList,
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
const ATTENTION_WIDGET = 'dots.goal.attention';
const HELP =
  "/goal [new IDEA | follow [ID [PROJECT]] | show [ID] | evidence TASK [EVENT|current] [PROJECT] | list | followed | offers | dismiss ID | refresh | unfollow ID | clear]. PROJECT is another project's exact key (the project field of dots task --json output); without it, follow uses this session's project. focus aliases follow; unfocus without an ID clears all. No shared task state is changed.";

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

/** Where attention is read and how shared events arrive; tests replace both. */
export interface AttentionOptions {
  readonly source?: AttentionSource;
  /** Undefined reads attention only on runs, opens and explicit refreshes. */
  readonly tasks?: Pick<Tasks, 'follow' | 'head'> | undefined;
}

export default function goalExtension(
  pi: ExtensionAPI,
  source: GoalSource = dotsSource,
  evidenceSource: EvidenceSource = dotsEvidenceSource,
  attentionOptions: AttentionOptions = {},
): void {
  const attentionSource = attentionOptions.source ?? dotsAttention();
  const attentionTasks =
    'tasks' in attentionOptions ? attentionOptions.tasks : new Tasks();
  const delivery = new TurnMessages(pi);
  const humans = new HumanInputs();
  /** Per session: scope, drafts and local outcomes end with it. */
  let attention: AttentionController | undefined;
  let attentionLine:
    {text: string | undefined; theme: Theme; redraw: () => void} | undefined;
  let attentionMounted = false;
  let lastView: PopupView | undefined;
  const background = (
    run: (controller: AttentionController) => Promise<void>,
  ) => {
    if (attention) attention.track(run(attention));
  };
  let focus: Focus | undefined;
  let model: GoalView | undefined;
  const followed = new Map<string, GoalView>();
  const dismissed = new Set<string>();
  let offers: GoalList | undefined;
  let discovery: AbortController | undefined;
  const offerKey = (project: string, id: string) =>
    JSON.stringify([project, id]);
  const candidates = () =>
    offers?.goals.filter(
      ({root}) =>
        !dismissed.has(offerKey(offers!.project, root.id)) &&
        followed.get(root.id)?.focus.project !== offers!.project,
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
  /** Attention scope is exactly the followed (project, goal) pairs. */
  const scope = (): GoalRef[] =>
    [...followed.values()].map(view => ({
      project: view.focus.project,
      id: view.focus.id,
    }));
  const persist = () => {
    pi.appendEntry(FOCUS_ENTRY, {
      version: 2,
      focus: focus ?? null,
      followed: [...followed.values()].map(view => view.focus),
    });
    background(a => a.setScope(scope()));
  };
  let generation = 0;
  let active = false;
  let request:
    | {
        controller: AbortController;
        pending: boolean;
        selected: boolean;
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
  let panel:
    | {
        view: GoalPanel;
        attention: AttentionView;
        theme: Theme;
        close: () => void;
      }
    | undefined;

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
          [...followed.values()],
          focus?.id,
          workerActivity,
          agent?.owner,
        ),
        summary.theme,
      );
      summary.redraw();
    }
    if (panel && attention)
      panel.attention.setModel(attention.model(), panel.theme);
    if (attentionLine) {
      const model = attention?.model();
      attentionLine.text = model && indicator(model);
      attentionLine.redraw();
    }
    if (panel && model) {
      panel.view.setGoal(
        {
          view: model,
          activity: workerActivity,
          owner: agent?.owner,
          followed: followed.size,
        },
        panel.theme,
      );
    }
  };
  const select = (ctx: ExtensionContext, chosen: GoalView) => {
    focus = chosen.focus;
    model = chosen;
    persist();
    draw(ctx);
  };
  /** A focused popup; closing it returns input to the editor with its draft. */
  const openPanel = (ctx: ExtensionContext, requested?: PopupView) => {
    if (!active || ctx.mode !== 'tui' || !ctx.hasUI || panel) return;
    const outstanding = attention?.model().items.length ?? 0;
    const view = requested ?? lastView ?? (outstanding ? 'attention' : 'work');
    background(a => a.refresh());
    if (!model) {
      notify(ctx, 'No focused goal. Use /goal follow ID or /goal list.');
      return;
    }
    const epoch = generation;
    ctx.ui
      .custom<void>(
        (tui, theme, bindings, done) => {
          if (!active || epoch !== generation) {
            done();
            return {render: () => [], invalidate() {}};
          }
          const shown: {popup?: GoalPopup} = {};
          const drafts = attention?.drafts ?? new Map<string, string>();
          const owned: NonNullable<typeof panel> = {
            attention: new AttentionView(
              bindings,
              {
                answer: (id, reply) => background(a => a.answer(id, reply)),
                dismiss: id => attention?.dismiss(id),
                drafts,
              },
              () => tui.terminal.rows,
              () => tui.requestRender(),
            ),
            view: new GoalPanel(
              bindings,
              {
                close: () => shown.popup?.back(),
                cycle: () => {
                  const views = [...followed.values()];
                  const at = views.findIndex(
                    view => view.focus.id === focus?.id,
                  );
                  select(ctx, views[(at + 1) % views.length]!);
                },
              },
              () => tui.requestRender(),
              () => popupBodyHeight(tui.terminal.rows),
            ),
            theme,
            close: () => done(),
          };
          const popup = new GoalPopup(
            {
              attention: owned.attention,
              work: owned.view,
              close: () => done(),
              viewed: shown => {
                lastView = shown;
              },
            },
            view,
          );
          popup.themeSource = () => theme;
          shown.popup = popup;
          panel = owned;
          paint();
          return Object.assign(popup, {
            dispose: () => {
              if (panel === owned) panel = undefined;
            },
          });
        },
        {overlay: true, overlayOptions: panelOverlay},
      )
      .catch((error: unknown) => {
        if (active && epoch === generation) notify(ctx, String(error), 'error');
      });
  };
  /** One quiet clickable line near the editor while a request awaits the person. */
  const drawAttention = (ctx: ExtensionContext) => {
    if (!active || ctx.mode !== 'tui' || !ctx.hasUI) return;
    const model = attention?.model();
    const text = model && indicator(model);
    if (!text) {
      if (attentionMounted) ctx.ui.setWidget(ATTENTION_WIDGET, undefined);
      attentionMounted = false;
      attentionLine = undefined;
      return;
    }
    if (attentionMounted) {
      paint();
      return;
    }
    attentionMounted = true;
    const epoch = generation;
    ctx.ui.setWidget(
      ATTENTION_WIDGET,
      (tui, theme) => {
        const owned = {text, theme, redraw: () => tui.requestRender()};
        attentionLine = owned;
        const region = new MouseRegion(
          {
            render: width =>
              active && epoch === generation && owned.text
                ? [
                    truncateToWidth(
                      theme.fg('warning', `${owned.text} · open`),
                      width,
                    ),
                  ]
                : [],
            invalidate() {},
          },
          event => {
            if (
              !active ||
              epoch !== generation ||
              dialog ||
              event.button !== 'left' ||
              event.shift ||
              event.alt ||
              event.ctrl
            )
              return undefined;
            if (event.type === 'press') return {handled: true, render: false};
            if (event.type !== 'click') return undefined;
            openPanel(ctx, 'attention');
            return {handled: true, render: false};
          },
        );
        return Object.assign(region, {
          dispose: () => {
            if (attentionLine === owned) {
              attentionLine = undefined;
              attentionMounted = false;
            }
          },
        });
      },
      {placement: 'belowEditor'},
    );
  };
  const draw = (ctx: ExtensionContext) => {
    if (!active || ctx.mode !== 'tui' || !ctx.hasUI) return;
    drawOffers(ctx);
    if (!model) {
      if (mounted) ctx.ui.setWidget(WIDGET, undefined);
      summary = undefined;
      mounted = false;
      panel?.close();
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
              openPanel(ctx);
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
    panel?.close();
    if (ctx && mounted && ctx.mode === 'tui' && ctx.hasUI)
      ctx.ui.setWidget(WIDGET, undefined);
    if (ctx && attentionMounted && ctx.mode === 'tui' && ctx.hasUI)
      ctx.ui.setWidget(ATTENTION_WIDGET, undefined);
    mounted = false;
    summary = undefined;
    attentionMounted = false;
    attentionLine = undefined;
  };

  // Ordinary observers share an in-flight read, including an explicit focus
  // change. Only explicit selection may supersede it. Observers follow that
  // replacement and consume its result, never the previous global model.
  // A selection rejects with its own failure; undefined means cancelled or superseded.
  // Each followed goal is read from its recorded project, not the session cwd.
  const refresh = async (
    ctx: ExtensionContext,
    selected?: {id: string; project?: string | undefined},
    signal?: AbortSignal,
  ): Promise<GoalView | undefined> => {
    const epoch = generation;
    if (!active || signal?.aborted) return undefined;
    if (selected || !request?.pending) {
      if (!selected && !focus) return undefined;
      request?.controller.abort();
      const read: NonNullable<typeof request> = {
        controller: new AbortController(),
        pending: true,
        selected: !!selected,
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
          const targets = selected ? [selected.id] : [...followed.keys()];
          const observations = await Promise.all(
            targets.map(async (target): Promise<GoalView> => {
              const saved = followed.get(target)?.focus;
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
          for (const observation of observations)
            followed.set(observation.focus.id, observation);
          if (selected) {
            focus = observations[0]!.focus;
            persist();
          }
          model = focus ? followed.get(focus.id) : undefined;
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
          // Pre-run observers refresh the followed set after selection settles.
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

  const restore = async (ctx: ExtensionContext) => {
    invalidate(ctx);
    workerActivity = undefined;
    active = true;
    focus = undefined;
    model = undefined;
    followed.clear();
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
      for (const item of saved.followed) followed.set(item.id, {focus: item});
    } catch (error) {
      notify(ctx, String(error), 'error');
    }
    attention?.bind(agent);
    background(a => a.setScope(scope()));
    const epoch = generation;
    if (focus) {
      model = {focus};
      draw(ctx);
      await refresh(ctx);
    }
    if (active && epoch === generation && ctx.mode === 'tui' && ctx.hasUI)
      await discover(ctx);
  };
  pi.on('input', event => {
    humans.observe(event);
  });
  pi.on('session_start', (event, ctx) => {
    agent = bindAgent(ctx.sessionManager.getSessionId(), event.reason);
    // Typed messages attest only within the session they were typed in.
    if (event.reason !== 'reload') humans.clear();
    delivery.bind(ctx);
    attention?.close();
    attention = new AttentionController({
      source: attentionSource,
      ...(attentionTasks ? {tasks: attentionTasks} : {}),
      deliver: (compose, delivered) => delivery.send(compose, delivered),
      branch: () => ctx.sessionManager.getBranch(),
      changed: () => {
        drawAttention(ctx);
        paint();
      },
      signal: signal => pi.events.emit(ATTENTION_EVENT, signal),
    });
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
    attention?.close();
    attention = undefined;
    delivery.close();
    pi.events.emit(ATTENTION_EVENT, {requests: []});
    stopWorkerActivity?.();
    stopWorkerActivity = undefined;
    workerActivity = undefined;
    invalidate();
    focus = undefined;
    model = undefined;
    followed.clear();
    offers = undefined;
  });
  pi.on('before_agent_start', async (event, ctx) => {
    const sections = event.systemPromptOptions.sections;
    for (const key of Object.keys(sections))
      if (key.startsWith(GOAL_SECTION_PREFIX)) delete sections[key];
    const startingEpoch = generation;
    if (ctx.mode === 'tui' && ctx.hasUI) await discover(ctx);
    if (
      !active ||
      startingEpoch !== generation ||
      (!focus && !request?.pending)
    )
      return;
    // Membership changes invalidate in-flight observations. Retry the remaining
    // set; an unfollow must neither resurrect a goal nor omit the other goals.
    while (active) {
      const epoch = generation;
      await refresh(ctx);
      if (!active) return;
      if (generation !== epoch) {
        if (followed.size || request?.pending) continue;
        return;
      }
      if (!followed.size) return;
      if (request?.selected) continue; // Explicit follow reads only its target.
      break;
    }
    const facts = await attention?.facts().catch(() => undefined);
    if (!active) return;
    for (const view of followed.values()) {
      Object.assign(sections, goalSections(view));
      const ref = {project: view.focus.project, id: view.focus.id};
      sections[`${goalSectionKey(view)}_attention`] = attentionContext(
        ref,
        facts?.get(`${ref.project}:${ref.id}`) ?? {
          goal: undefined,
          decisions: undefined,
          mandateRequest: undefined,
          failure: 'attention not read',
        },
        agent?.owner,
      );
    }
  });
  pi.on('agent_end', async (_event, ctx) => {
    const epoch = generation;
    background(a => a.refresh());
    await refresh(ctx);
    if (active && epoch === generation && ctx.mode === 'tui' && ctx.hasUI)
      await discover(ctx);
  });

  /** TUI: open the panel, then refresh it. Headless: one inspection message. */
  const show = async (ctx: ExtensionContext) => {
    const epoch = generation;
    if (ctx.mode === 'tui' && ctx.hasUI) {
      openPanel(ctx);
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
    name: 'attention_record_chat',
    label: 'Record chat answer',
    description: RECORD_CHAT_DESCRIPTION,
    parameters: RecordChatParameters,
    annotations: {
      readOnlyHint: false,
      destructiveHint: false,
      idempotentHint: false,
      openWorldHint: false,
    },
    async execute(_callId, params, signal) {
      const actor = agent;
      if (!actor) throw new Error('No live Pi session to record for.');
      try {
        const text = await recordChat(
          params,
          actor,
          humans,
          attentionSource,
          scope(),
          request => attention?.projectOf(request),
          signal ?? new AbortController().signal,
        );
        background(a => a.refresh());
        return {content: [{type: 'text', text}], details: {kind: params.kind}};
      } catch (error) {
        throw new Error(attentionFailure(error), {cause: error});
      }
    },
  });
  pi.registerShortcut(PANEL_KEY, {
    description: 'Toggle the goal popup',
    handler: ctx => {
      if (panel) panel.close();
      else openPanel(ctx);
    },
  });

  pi.registerTool({
    name: 'goal_focus',
    label: 'Focus goal',
    description:
      'Use after creating a goal or when coordinating an existing one so it appears in this session’s UI and context. Follow an existing goal in this session, additively, and select its details. Every followed goal gets fresh brief/task context before runs. Other goals remain followed; no tasks, claims, or execution state change. Use goal_unfocus to remove one or clear all. Create shared goals with dots task goal create TITLE --brief PATH --json.',
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
          content: [{type: 'text', text: orientation(observed)}],
          details: {id: observed.focus.id, project: observed.focus.project},
        };
      } finally {
        if (dialog === controller) dialog = undefined;
      }
    },
  });

  const unfollow = (ctx: ExtensionContext, id?: string) => {
    invalidate(ctx);
    for (const view of followed.values()) {
      if (!id || view.focus.id === id)
        dismiss(view.focus.project, view.focus.id);
    }
    if (id) followed.delete(id);
    else followed.clear();
    if (!focus || !followed.has(focus.id))
      focus = followed.values().next().value?.focus;
    model = focus ? followed.get(focus.id) : undefined;
    persist();
    draw(ctx);
    return id
      ? `Unfollowed ${id}; shared goals, tasks and claims are unchanged.`
      : 'Goal unfocused; all session follows cleared. Shared goals, tasks and claims are unchanged.';
  };
  pi.registerTool({
    name: 'goal_unfocus',
    label: 'Unfollow goal',
    description:
      "Remove one goal from this session's followed set, or clear all follows on request. Removes its UI and future automatic context, not prior conversation. Does not change shared goals, tasks, claims, workers or other sessions. Specify id or all:true.",
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
        details: {followed: [...followed.keys()]},
      };
    },
  });
  pi.registerTool({
    name: 'goal_list',
    label: 'List followed goals',
    description:
      "List this session's followed goal IDs and the selected details view. Read-only session state; use dots task goal list --json for all shared project goals. Selection is not execution state.",
    parameters: Type.Object({}),
    annotations: {
      readOnlyHint: true,
      destructiveHint: false,
      idempotentHint: true,
      openWorldHint: false,
    },
    async execute() {
      const goals = [...followed.values()].map(view => ({
        ...view.focus,
        title: view.snapshot?.overview.goal.root.title,
        selected: view.focus.id === focus?.id,
      }));
      return {
        content: [{type: 'text', text: JSON.stringify({followed: goals})}],
        details: {followed: goals},
      };
    },
  });

  pi.registerCommand('goal', {
    description:
      'Follow multiple goals; without arguments, open the goal popup: [new IDEA|follow [ID [PROJECT]]|show [ID]|evidence TASK [EVENT|current] [PROJECT]|list|followed|offers|dismiss ID|refresh|unfollow ID|clear]',
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
            'Discuss only what is unclear. When there is enough direction, capture a concise Markdown brief (including our working agreement) in a suitable document, then use dots task goal create TITLE --brief PATH --json to create its root and association together. If an appropriate root already exists, attach the brief instead. Add useful initial slices under that root and call goal_focus with its id so I can see it here.',
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
          followed.size
            ? [...followed.values()]
                .map(
                  view =>
                    `${view.focus.id === focus?.id ? '> ' : '  '}${view.focus.id} ${view.snapshot?.overview.goal.root.title ?? ''} · ${compactSummary(view)}`,
                )
                .join('\n')
            : 'No followed goals.',
          'info',
          'goal_list for complete session follows',
        );
        return;
      }
      if (action === 'show') {
        if (words[1]) {
          const chosen = followed.get(words[1]);
          if (!chosen) {
            notify(
              ctx,
              'Goal is not followed. Use /goal follow ID first.',
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
        background(a => a.refresh());
        await refresh(ctx);
        if (!active || generation !== epoch) return;
        while (request?.selected && followed.size) {
          await refresh(ctx);
          if (!active || generation !== epoch) return;
        }
        const failing = [...followed.values()].filter(
          view => view.error || view.brief?.warning,
        );
        const status = followed.size
          ? `${followed.size} followed goal(s) refreshed (local records only).`
          : 'No followed goals.';
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
                        `${g.root.id} [${g.root.status}]${followed.has(g.root.id) ? ' [followed]' : ''} ${g.root.title}`,
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
            `Focused ${id}; following ${followed.size} goal(s). Other goals are unchanged. No work started. Use /goal to inspect.`,
          );
      } catch (error) {
        if (current()) notify(ctx, String(error), 'error');
      } finally {
        if (dialog === controller) dialog = undefined;
      }
    },
  });
}
