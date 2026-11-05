// An in-memory attention store with the CLI's request lifecycle, for tests
// that need no `dots` process. IDs, projects and DTO fields match the store's.

export const PROJECT = `p-${'a'.repeat(32)}`;
export const requestId = n => `r-${String(n).padStart(32, '0')}`;
export const decisionId = n => `d-${String(n).padStart(32, '0')}`;

export function request(overrides = {}) {
  return {
    id: requestId(1),
    project: PROJECT,
    goal: 't-123abc',
    key: 'scope',
    kind: 'decision',
    title: 'Choose the rollout scope',
    question: 'Should the first slice cover every project or only this one?',
    context: 'Every project needs the catalog migration first.',
    recommendation: 'This project only',
    choices: [
      {id: 'one', label: 'This project only', detail: 'Smaller, reversible'},
      {id: 'all', label: 'Every project', detail: null},
    ],
    tasks: [{id: 't-456def', title: 'Migrate the catalog', status: 'open'}],
    state: 'open',
    created_at: 100,
    created_by: 'pi-coordinator-0000000000000000',
    supersedes: null,
    superseded_by: null,
    answer: null,
    closure: null,
    ...overrides,
  };
}

export function decision(overrides = {}) {
  return {
    id: decisionId(1),
    project: PROJECT,
    goal: 't-123abc',
    kind: 'decision',
    decision: 'Ship behind a flag.',
    human: {
      text: 'ship it behind a flag',
      source: 'chat',
      provenance: 'pi session s · interactive input',
    },
    request: null,
    at: 50,
    by: 'pi-coordinator-0000000000000000',
    ...overrides,
  };
}

/** A fake AttentionSource; `goals` maps "project:goal" to goal records. */
export function attentionStore({
  coordinator = null,
  status = 'open',
  requests = [],
  decisions = [],
  mandate = null,
} = {}) {
  const state = {
    coordinator,
    status,
    requests: new Map(requests.map(item => [item.id, structuredClone(item)])),
    decisions: [...decisions],
    mandate,
    calls: [],
    fail: undefined,
  };
  const goalRecord = ref => ({
    project: ref.project,
    project_name: 'fixture',
    goal: ref.id,
    title: 'Fixture goal',
    status: state.status,
    coordinator: state.coordinator,
    mandate: state.mandate,
    requests: [...state.requests.values()].filter(
      item => item.goal === ref.id && item.state !== 'closed',
    ),
  });
  const source = {
    async list(goals, signal) {
      state.calls.push({op: 'list', goals, signal});
      if (state.fail) throw new Error(state.fail);
      return structuredClone({goals: goals.map(goalRecord), errors: []});
    },
    async decisions(goals) {
      state.calls.push({op: 'decisions', goals});
      return {
        goals: goals.map(ref => ({
          project: ref.project,
          project_name: 'fixture',
          goal: ref.id,
          title: 'Fixture goal',
          decisions: state.decisions.filter(item => item.goal === ref.id),
        })),
        errors: [],
      };
    },
    async show(project, id) {
      state.calls.push({op: 'show', project, id});
      const found = state.requests.get(id);
      if (!found) throw new Error(`request ${id} not found`);
      return structuredClone(found);
    },
    async answer(project, id, content, actor) {
      state.calls.push({op: 'answer', project, id, content, actor});
      await Promise.resolve();
      const found = state.requests.get(id);
      if (!found) throw new Error(`request ${id} not found`);
      if (found.state !== 'open')
        throw new Error(`request ${id} is ${found.state}, not open`);
      found.state = 'answered';
      found.answer = {
        choice: content.choice ?? null,
        text: content.text ?? null,
        source: content.source,
        provenance: content.provenance,
        interpretation: content.interpretation ?? null,
        at: 200,
        by: actor.env.DOTS_AGENT_OWNER ?? null,
      };
      return structuredClone(found);
    },
    async decide(goal, content, actor) {
      state.calls.push({op: 'decide', goal, content, actor});
      const recorded = decision({
        id: decisionId(state.decisions.length + 10),
        goal: goal.id,
        kind: content.kind,
        decision: content.decision,
        human: content.human,
        by: actor.env.DOTS_AGENT_OWNER ?? null,
      });
      state.decisions.push(recorded);
      if (content.kind === 'mandate') state.mandate = recorded;
      return recorded;
    },
  };
  /** Replace an open request, as `dots attention supersede` does. */
  const supersede = (id, replacement) => {
    const old = state.requests.get(id);
    old.state = 'closed';
    old.superseded_by = replacement.id;
    old.closure = {
      kind: 'superseded',
      at: 150,
      by: null,
      reason: null,
      decision: null,
    };
    state.requests.set(replacement.id, {...replacement, supersedes: id});
  };
  return {state, source, supersede};
}
