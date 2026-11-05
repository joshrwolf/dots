import {VStack} from '@earendil-works/pi-tui';
import type {Scenario} from './scenarios.ts';
import {goalFixtures} from './goals.ts';
import {Inspector} from '../src/index.ts';
import {GoalPanel} from '../extensions/goal/panel.ts';
import {
  AttentionView,
  GoalPopup,
  popupBodyHeight,
} from '../extensions/goal/attention/panel.ts';
import {
  attentionModel,
  type ItemState,
} from '../extensions/goal/attention/model.ts';
import type {
  AttentionList,
  Request,
} from '../extensions/goal/attention/source.ts';

const PROJECT = `p-${'0'.repeat(32)}`;
const OWNER = 'pi-fixture-0000000000000000';

function request(n: number, overrides: Partial<Request> = {}): Request {
  return {
    id: `r-${String(n).padStart(32, '0')}`,
    project: PROJECT,
    goal: 't-0f1x',
    key: `fixture-${n}`,
    kind: 'decision',
    title: 'Choose the rollout scope',
    question: 'Should the first slice cover every project or only this one?',
    context: 'Every project needs the catalog migration first.',
    recommendation: 'This project only',
    choices: [
      {id: 'one', label: 'This project only', detail: 'Smaller, reversible'},
      {
        id: 'all',
        label: 'Every project',
        detail: 'One migration window for the whole catalog',
      },
    ],
    tasks: [{id: 't-0f2x', title: 'Migrate the catalog', status: 'open'}],
    state: 'open',
    created_at: 100 + n,
    created_by: OWNER,
    supersedes: null,
    superseded_by: null,
    answer: null,
    closure: null,
    ...overrides,
  };
}

const goal = (
  id: string,
  title: string,
  coordinator: string,
  requests: Request[],
): AttentionList['goals'][number] => ({
  project: PROJECT,
  project_name: 'fixture',
  goal: id,
  title,
  status: 'open',
  coordinator,
  mandate: null,
  requests,
});

const first = request(1);
const second = request(2, {
  goal: 't-0f3x',
  title: 'Approve the public release notes',
  question: 'The draft names every changed command. Publish it as written?',
  context: null,
  recommendation: null,
  choices: [
    {id: 'yes', label: 'Publish as written', detail: null},
    {id: 'edit', label: 'Shorten the command list first', detail: null},
  ],
  tasks: [],
});
const replacement = request(3, {
  supersedes: first.id,
  question: 'Should the first slice cover this project and its fixtures?',
});

/** Phases: what the session's goals (coordinated and followed) hold. */
const phases: readonly {
  readonly label: string;
  readonly list: AttentionList | undefined;
  readonly local?: Map<
    string,
    {state: ItemState; request: Request; goal: string}
  >;
}[] = [
  {
    label: 'coordinated and followed requests',
    list: {
      goals: [
        goal('t-0f1x', 'Export interface', OWNER, [first]),
        goal('t-0f3x', 'Release notes', 'pi-other-0000000000000000', [second]),
      ],
      errors: [],
    },
  },
  {
    label: 'answer saving',
    list: {
      goals: [goal('t-0f1x', 'Export interface', OWNER, [first])],
      errors: [],
    },
    local: new Map([
      [
        first.id,
        {state: {kind: 'saving'}, request: first, goal: 'Export interface'},
      ],
    ]),
  },
  {
    label: 'replaced while drafting',
    list: {
      goals: [goal('t-0f1x', 'Export interface', OWNER, [replacement])],
      errors: [],
    },
    local: new Map([
      [
        first.id,
        {
          state: {kind: 'stale', replacement},
          request: {...first, state: 'closed', superseded_by: replacement.id},
          goal: 'Export interface',
        },
      ],
    ]),
  },
  {
    label: 'nothing needs an answer',
    list: {goals: [goal('t-0f1x', 'Export interface', OWNER, [])], errors: []},
  },
  {
    label: 'unavailable',
    list: {
      goals: [],
      errors: [{project: PROJECT, goal: 't-0f1x', error: 'store is locked'}],
    },
  },
];

/** The shipping goal popup's Attention view with in-memory requests and actions. */
export const attentionScenario: Scenario = {
  id: 'attention',
  title: 'Goal attention',
  phases: phases.map(phase => phase.label),
  create(context) {
    const preview = new Inspector(
      context.bindings,
      context.close,
      context.redraw,
      context.height,
    );
    const rows = () => context.height?.() ?? 30;
    const drafts = new Map<string, string>();
    // Each phase is its own recorded state: selection never carries over.
    const open = () => {
      const attention = new AttentionView(
        context.bindings,
        {answer() {}, dismiss() {}, drafts},
        rows,
        context.redraw,
      );
      const shown: {popup?: GoalPopup} = {};
      const work = new GoalPanel(
        context.bindings,
        {close: () => shown.popup?.back(), cycle() {}},
        context.redraw,
        () => popupBodyHeight(rows()),
      );
      shown.popup = new GoalPopup(
        {attention, work, close() {}, viewed() {}},
        'attention',
      );
      return {attention, work, popup: shown.popup};
    };
    let {attention, work, popup} = open();
    let shownPhase = 0;
    let expanded = false;
    const component = Object.assign(new VStack(), {
      handleInput(data: string) {
        (expanded ? popup : preview).handleInput(data);
      },
    });
    return {
      component,
      update(phase, nextExpanded, theme) {
        expanded = nextExpanded;
        const state = phases[phase]!;
        if (phase !== shownPhase) ({attention, work, popup} = open());
        shownPhase = phase;
        if (state.local?.has(first.id)) drafts.set(first.id, 'Only this one');
        else drafts.clear();
        popup.themeSource = () => theme;
        work.setGoal({view: goalFixtures(3)[0]!, goals: 2}, theme);
        attention.setModel(
          attentionModel(state.list, OWNER, state.local ?? new Map()),
          theme,
        );
        component.clear();
        preview.setContent(
          {
            title: 'Goal attention',
            detail: expanded ? 'popup keys active' : state.label,
            body: popup,
            hint: expanded
              ? '↑/↓ select · Enter choose · ←/→ view · E release keys'
              : 'Space state · E use popup keys · T theme · Esc close',
          },
          theme,
        );
        component.addChild(preview);
      },
    };
  },
};
