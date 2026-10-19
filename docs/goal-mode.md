# Goal mode

Goal mode is an opt-in workspace for a multi-slice goal, not a required workflow
or a fixed agent pipeline. It combines a durable brief, the existing task list,
and a plain-language working agreement. Models choose the strategy; tools keep
context and coordination reliable.

## Direction

- A goal adopts an existing root task. Its identity and slices stay in the
  shared task store; there is no parallel progress database.
- A Markdown brief captures the outcome, non-goals, design guidance, reference
  patterns, and uncertainties as needed—not a mandatory template. It links to
  canonical material rather than duplicating it.
- Brief references do not depend on the current memory implementation. Memory
  remains shared context; transcripts are not the only place decisions survive.
- Focus, execution permission, and progress are separate. Creating or inspecting
  a goal never authorizes execution. A working agreement can range from
  “do one slice and return” to “continue within the agreed scope.” It lives in
  the brief or conversation, not a separate run object or execution protocol.
- Agents choose how to advance authorized work using normal tool calls, workers,
  and wakes. Goal mode supplies context and visibility, not an automatic
  re-prompting loop, execution presets, or arbitrary limits on model behavior.
- Claims remain atomic. Worker completion is not proof of verification or landing.
- Git and GitHub remain evidence about code. Local task summaries describe
  recorded state, not freshly verified PR status.
- Periodic whole-result reviews and maintained reference patterns provide
  cohesion without a formal decision state machine.

## Boundaries

Shared goal operations belong with the task foundation and are exposed through
`dots`. Pi consumes that interface rather than writing its database directly.
Pi owns session focus, presentation, and context restoration. Existing workers
and wakes remain available to agents without a goal-specific execution layer.
Durable goal state stays independent of live runtimes.

Presentation uses the existing Pi UI foundation: readonly observations, shared
components, and host-owned lifecycles. Rendering neither edits tasks nor starts
execution. Initial autonomous work requires a live Pi session; a durable runner,
workflow DSL, generic plugin framework, and automatic merging are out of scope.

## First proving ground

Capture a brief, associate slices, inspect progress, execute one slice, record
its result, and resume from another session. Express the desired involvement in
the brief or conversation and let the agent proceed accordingly. Use a real goal
to discover gaps before considering additional execution machinery.
