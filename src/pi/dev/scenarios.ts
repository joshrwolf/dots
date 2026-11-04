import {
  getSelectListTheme,
  UserMessageComponent,
  type KeybindingHints,
  type KeybindingsManager,
  type Theme,
} from '@earendil-works/pi-coding-agent';
import {
  SelectList,
  Text,
  Spacer,
  VStack,
  sliceByColumn,
  truncateToWidth,
  visibleWidth,
  type Component,
  type Keybinding,
  type TUI,
} from '@earendil-works/pi-tui';
import {
  Inspector,
  OutputView,
  Panel,
  PriorityRow,
  type ToolObservation,
  type ToolPresentation,
} from '../src/index.ts';
import {transcript} from './transcript.ts';
import {workerInspectorScenario} from './workers.ts';
import {thinkingComponent} from './thinking.ts';
import {goalActivityFixture, goalFixtures} from './goals.ts';
import {
  GoalPanel,
  PANEL_KEY,
  panelHeight,
  panelWidth,
  summaryRow,
} from '../extensions/goal/panel.ts';
import {bashPresentation} from '../src/features/tools/bash.ts';
import {readPresentation} from '../src/features/tools/read.ts';
import {writePresentation} from '../src/features/tools/write.ts';
import {editPresentation} from '../src/features/tools/edit.ts';
import {codemodePresentation} from '../src/features/tools/codemode.ts';
import {FooterView} from '../src/features/frame.ts';
import {JobInspector} from '../extensions/jobs/inspector.ts';
import {emptyFacts, type Job} from '../extensions/jobs/protocol.ts';
import {
  JobsWidget,
  canStop,
  projectJob,
  jobsStatus,
} from '../extensions/jobs/present.ts';
import {renderWake} from '../extensions/wake/present.ts';
import {
  createWakeWidget,
  wakeActivityBody,
  type WakeActivity,
} from '../extensions/wake/activity.ts';
import {
  createWidget,
  renderEvent,
  WORKER_FRAME_MS,
  type WidgetRow,
} from '../extensions/workers/present.ts';

// Hand-authored data only. No production runtime, process, file or API access.
// The clock advances presentation-only spinner frames; observations stay fixed.
const animationFrame = () => Math.floor(Date.now() / WORKER_FRAME_MS);
export interface ScenarioContext {
  readonly tui: TUI;
  readonly bindings: Pick<KeybindingsManager, 'matches' | 'getKeys'>;
  readonly close: () => void;
  readonly redraw: () => void;
  readonly height?: () => number;
  readonly createEditor?: (theme: Theme) => Component;
}
/** Renderer hints from the overlay's own bindings; fixtures have no render context. */
function rendererHints(
  bindings: Pick<KeybindingsManager, 'getKeys'>,
  theme: Theme,
): KeybindingHints {
  const keyText = (keybinding: Keybinding) =>
    bindings.getKeys(keybinding).join('/');
  return {
    keyText,
    keyHint: (keybinding, description) =>
      theme.fg('dim', keyText(keybinding)) +
      theme.fg('muted', ` ${description}`),
  };
}
export interface ScenarioView {
  readonly component: Component;
  update(phase: number, expanded: boolean, theme: Theme): void;
  readonly interactive?: boolean;
  dispose?(): void;
  canAct?(): boolean;
  act?(): number | undefined;
}
export interface Scenario {
  readonly id: string;
  readonly title: string;
  readonly phases: readonly string[];
  readonly create: (context: ScenarioContext, theme: Theme) => ScenarioView;
}
type Frame = {readonly label: string; readonly observation: ToolObservation};
const call = (
  args: unknown,
  argsComplete = true,
  executionStarted = true,
): ToolObservation => ({
  slot: 'call',
  cwd: '/workspace/example',
  args,
  argsComplete,
  executionStarted,
});
const result = (
  args: unknown,
  text: string,
  details: unknown = undefined,
  error = false,
  partial = false,
): ToolObservation => ({
  slot: 'result',
  cwd: '/workspace/example',
  args,
  content: [{type: 'text', text}],
  details,
  error,
  partial,
  durationMs: partial ? undefined : 2400,
  showImages: false,
});
function tool<Model>(
  id: string,
  title: string,
  spec: ToolPresentation<Model>,
  frames: readonly Frame[],
): Scenario {
  return {
    id,
    title,
    phases: frames.map(frame => frame.label),
    create(context) {
      const view = spec.create(context.redraw);
      const inspector = new Inspector(
        context.bindings,
        context.close,
        context.redraw,
        context.height,
      );
      return {
        component: inspector,
        update(phase, expanded, theme) {
          const frame = frames[phase]!;
          view.update(spec.project(frame.observation), theme, {
            expanded,
            ...rendererHints(context.bindings, theme),
          });
          inspector.setContent(
            {
              title,
              detail: frame.label,
              body: view,
              hint: 'Space next state · E expand/collapse · ↑/↓ scroll · Esc close',
            },
            theme,
          );
        },
      };
    },
  };
}
const path = '/workspace/example/src/日本語/report.ts';
const command = 'npm test -- --reporter=spec';
const shell = {command};
const longCommand = {
  command:
    'pnpm --filter @example/shipping-component test -- --reporter=verbose ' +
    '--fixture=packages/shipping-component/src/日本語/preview.ts ' +
    '--check-emoji=👩🏽‍💻 --show-metadata --include=all',
};
const source =
  'export function greet(name: string) {\n  return `Hello, ${name} 👩🏽‍💻`;\n}\n';
const editArgs = {path, edits: [{oldText: 'Hello', newText: 'Welcome'}]};
const script = {
  code: 'const results = await Promise.all([tools.read({path: "report.ts"}), tools.bash({command: "npm test"})]);\ntext(results);',
};
const nested = (status: string) => ({
  calls: [
    {id: 'demo/1', name: 'read', args: '{"path":"report.ts"}', status: 'ok'},
    {
      id: 'demo/2',
      name: 'bash',
      args: '{"command":"npm test"}',
      status,
      ...(status === 'error' ? {error: 'exit 1'} : {}),
    },
  ],
});

export function jobFixture(phase = 1): Job {
  const job: Job = {
    version: 1,
    id: 'j-00000000-0000-0000-0000-000000000001',
    command,
    cwd: '/workspace/example',
    directory: '/fixture/jobs/example',
    timeoutMs: 60000,
    startedAt: 1000,
    finishedAt: null,
    state: 'running',
    stopIntent: null,
    facts: emptyFacts(),
    supervisorExit: null,
    observationLoss: null,
    artifactsEvicted: false,
  };
  if (phase === 0) job.state = 'accepted';
  if (phase === 2) {
    job.state = 'stopping';
    job.stopIntent = 'stop';
    job.facts.reason = 'stop';
    job.facts.termSent = true;
  }
  if (phase === 3 || phase === 4) {
    job.state = 'finished';
    job.finishedAt = 3400;
    job.facts.root = {code: phase === 4 ? 1 : 0, signal: null, error: null};
    job.supervisorExit = {code: 0, signal: null};
    job.facts.capture.streamsEnded = true;
    job.facts.capture.prefixFlushed = true;
    job.facts.capture.tailFlushed = true;
    if (phase === 4) {
      job.facts.capture.capped = true;
      job.facts.capture.droppedBytes = 1024;
    }
  }
  if (phase === 5) {
    job.state = 'unknown';
    job.observationLoss =
      'Supervisor observation lost; historical jobs cannot be stopped.';
  }
  return job;
}

const thinkingScenario: Scenario = {
  id: 'thinking',
  title: 'Thinking blocks',
  phases: ['summary', 'structured detail', 'streaming'],
  create(context) {
    const inspector = new Inspector(
      context.bindings,
      context.close,
      context.redraw,
      context.height,
    );
    return {
      component: inspector,
      update(phase, expanded, theme) {
        inspector.setContent(
          {
            title: 'Thinking blocks',
            detail: 'native assistant component · E toggles thinking',
            body: thinkingComponent(phase, !expanded),
          },
          theme,
        );
      },
    };
  },
};

/** Fixture screen: the transcript above the editor and its summary line, with
 * the popup composited at Pi's centered overlay geometry. */
class GoalScreen extends VStack {
  private readonly transcript: Component;
  private readonly dock: Component;
  private readonly rows: () => number;
  private popup: Component | undefined;
  constructor(transcript: Component, dock: Component, rows: () => number) {
    super();
    this.transcript = transcript;
    this.dock = dock;
    this.rows = rows;
  }
  show(popup: Component | undefined): void {
    this.popup = popup;
    this.invalidate();
  }
  override render(width: number): string[] {
    if (width <= 0) return [];
    const rows = this.rows();
    const dock = this.dock.render(width);
    const transcript = this.transcript.render(width);
    const screen = [
      ...transcript.slice(0, Math.max(0, rows - dock.length)),
      ...Array<string>(
        Math.max(0, rows - dock.length - transcript.length),
      ).fill(''),
      ...dock,
    ];
    if (!this.popup) return screen;
    const columns = panelWidth(width);
    const popup = this.popup.render(columns).slice(0, rows);
    const top = Math.floor((rows - popup.length) / 2);
    const left = Math.floor((width - columns) / 2);
    return screen.map((line, i) => {
      const own = popup[i - top];
      if (own === undefined) return line;
      const head = sliceByColumn(line, 0, left, true);
      return (
        head +
        ' '.repeat(Math.max(0, left - visibleWidth(head))) +
        '\x1b[0m' +
        truncateToWidth(own, columns, '', true) +
        '\x1b[0m' +
        sliceByColumn(line, left + columns, width - left - columns, true)
      );
    });
  }
}

function goalScenario(): Scenario {
  const states = [
    [8, 'nested groups'],
    [11, 'assigned live worker'],
    [12, 'worker asking'],
    [13, 'worker quiet'],
    [9, 'closed history'],
    [10, 'incomplete snapshot'],
    [0, 'claimed'],
    [1, 'held'],
    [2, 'recorded complete'],
    [3, 'multiple goals'],
    [4, 'unavailable'],
    [5, 'loading'],
    [6, 'brief warning'],
    [7, 'empty'],
  ] as const;
  // The fixture's claim owner stands in for this session.
  const owner = 'fixture-reviewer';
  return {
    id: 'goal',
    title: 'Goal popup',
    phases: states.map(([, label]) => label),
    create(context) {
      const preview = new Inspector(
        context.bindings,
        context.close,
        context.redraw,
        context.height,
      );
      const screen = () => context.height?.() ?? 20;
      const panel = new GoalPanel(
        context.bindings,
        {close() {}, cycle() {}},
        context.redraw,
        () => panelHeight(screen()),
      );
      const summary = new PriorityRow();
      let expanded = false;
      const component = Object.assign(new VStack(), {
        handleInput(data: string) {
          (expanded ? panel : preview).handleInput(data);
        },
      });
      return {
        component,
        update(phase, nextExpanded, theme) {
          expanded = nextExpanded;
          const views = goalFixtures(states[phase]![0]);
          const activity = goalActivityFixture(
            states[phase]![0],
            animationFrame(),
          );
          summary.update(
            summaryRow(views, views[0]!.focus.id, activity, owner),
            theme,
          );
          panel.setGoal(
            {view: views[0]!, activity, owner, followed: views.length},
            theme,
          );
          const frame = new GoalScreen(
            new VStack([
              new UserMessageComponent(
                'Coordinate the export interface goal.',
                undefined,
                0,
              ),
              new Spacer(1),
              new Text(
                theme.fg('text', 'I’ll check the remaining slices first.'),
                0,
                0,
              ),
            ]),
            new VStack([
              new Spacer(1),
              ...(context.createEditor ? [context.createEditor(theme)] : []),
              summary,
            ]),
            screen,
          );
          frame.show(expanded ? panel : undefined);
          component.clear();
          preview.setContent(
            {
              title: 'Goal popup',
              detail: expanded
                ? `popup open · ${PANEL_KEY} toggles`
                : 'summary line',
              body: frame,
              hint: expanded
                ? 'Space state · E hide popup · 1/2/3 · h · ↑/↓ select · ←/→ fold · Esc close'
                : 'Space state · E show popup · T theme · ↑/↓ scroll · Esc close',
            },
            theme,
          );
          component.addChild(preview);
        },
      };
    },
  };
}

function wakeFixtures(phase: number): WakeActivity[] {
  return Array.from({length: phase >= 2 ? 5 : 2}, (_, i): WakeActivity =>
    i === 4
      ? {
          kind: 'task',
          id: 'k-fixture-task',
          text: 'Review the build once it is ready',
          task: 't-0f1x',
          title: 'Fixture build',
          project: 'fixture',
          until: ['review'],
          deadline: 23 * 3_600_000,
          cursor: 12,
          disconnected: 'dots task events exited with code 1',
          fired: 0,
          limit: 1,
          state: 'waiting',
        }
      : {
          kind: 'time' as const,
          id: `k-fixture-${i}`,
          text: i
            ? `Inspect check result ${i}`
            : 'Review the latest verification result',
          at: (i + 1) * 120000,
          every: i ? undefined : 120000,
          fired: i ? 0 : 2,
          limit: i ? 1 : 10,
          check: i ? undefined : 'fixture-check',
          on: 'change',
          lastCode: phase && !i ? 1 : undefined,
          state: phase === 1 ? (i ? 'queued' : 'checking') : 'scheduled',
        },
  );
}

export const scenarios: readonly Scenario[] = [
  {
    id: 'inspection-shell',
    title: 'Inspection shell',
    phases: ['picker', 'empty', 'long title'],
    create(context) {
      const panel = new Panel();
      const items = Array.from({length: 32}, (_, index) => ({
        value: `worker-${index + 1}`,
        label: index === 0 ? 'review 日本語 👩🏽‍💻' : `review-${index + 1}`,
        description:
          index % 3 === 0
            ? 'asking · read-only · owner example'
            : 'running · owned',
      }));
      const picker = new SelectList(
        items,
        Math.max(1, (context.height?.() ?? 22) - 3),
        getSelectListTheme(),
      );
      picker.onCancel = context.close;
      const component = Object.assign(new VStack([panel]), {
        handleInput(data: string) {
          picker.handleInput(data);
          context.redraw();
        },
      });
      return {
        component,
        interactive: true,
        update(phase, _expanded, theme) {
          panel.update(
            {
              heading: {
                start: [
                  {
                    text:
                      phase === 2
                        ? 'Workers · a long heading 日本語 👩🏽‍💻'
                        : 'Workers',
                    tone: 'toolTitle',
                    bold: true,
                    priority: 100,
                    shrink: 'clip',
                  },
                ],
                end: [
                  {
                    text: '32 retained',
                    tone: 'muted',
                    priority: 10,
                    shrink: 'omit',
                  },
                ],
              },
              body:
                phase === 1 ? new Text('No retained workers.', 0, 0) : picker,
              footer: {
                start: [
                  {
                    text: '↑/↓ select · Ctrl+Space state · Esc close',
                    tone: 'dim',
                    priority: 100,
                    shrink: 'clip',
                  },
                ],
                end: [],
              },
            },
            theme,
          );
        },
      };
    },
  },
  {
    id: 'frame',
    title: 'Application frame',
    phases: [
      'combined activity',
      'context warning',
      'context critical',
      'unknown context',
      'long labels',
      'publisher overflow',
      'no model',
      'active work',
      'activity overflow',
      'settled activity',
    ],
    create(context) {
      const inspector = new Inspector(
        context.bindings,
        context.close,
        context.redraw,
        context.height,
      );
      const footer = new FooterView();
      const jobs = new JobsWidget();
      return {
        component: inspector,
        update(phase, expanded, theme) {
          const combined = phase === 0 || phase >= 7;
          const workers: WidgetRow[] =
            phase === 9
              ? []
              : combined
                ? Array.from({length: phase === 8 ? 8 : 2}, (_, i) => ({
                    label: i ? `build-${i}` : 'review',
                    model: 'example/example-model',
                    thinking: i ? 'medium' : 'high',
                    context: 25,
                    state: i ? 'running' : 'asking',
                    depth: 0,
                    elapsed: '1m',
                    calls: 4,
                    tokens: 2000,
                    detail: i
                      ? 'read src/report.ts'
                      : 'Which revision should I compare?',
                    idleMs: 0,
                  }))
                : phase === 1
                  ? [
                      {
                        label: 'review',
                        state: 'asking',
                        depth: 0,
                        elapsed: '1m',
                        calls: 4,
                        tokens: 2000,
                        detail: 'Which revision should I compare?',
                        idleMs: 0,
                      },
                    ]
                  : [];
          const jobModels =
            combined && phase !== 9
              ? Array.from({length: phase === 8 ? 5 : 1}, (_, i) =>
                  projectJob({
                    ...jobFixture(i === 0 && phase === 8 ? 5 : 1),
                    id: `j-fixture-${i}`,
                    command: `check package-${i}`,
                  }),
                )
              : [];
          jobs.update(jobModels, theme);
          footer.update(
            {
              project: phase === 4 ? '日本語-project-👩🏽‍💻' : 'example',
              branch:
                phase === 6
                  ? undefined
                  : phase === 4
                    ? 'feature/a-long-branch-for-responsive-status-preview'
                    : 'feature/presentation',
              model:
                phase === 6
                  ? 'no model'
                  : phase === 4
                    ? 'example-provider/long-reasoning-model-name'
                    : 'example-model',
              thinking: phase === 6 ? 'off' : 'high',
              contextPercent:
                phase === 3 || phase === 6
                  ? undefined
                  : phase === 1
                    ? 80
                    : phase === 2
                      ? 95
                      : 25,
              statuses: [
                ...(jobModels.length
                  ? [{key: 'dots.jobs', text: jobsStatus(jobModels)!}]
                  : []),
                ...(combined || phase === 5
                  ? [
                      {
                        key: 'example.notice',
                        text: 'Companion update · /reload when idle',
                      },
                    ]
                  : []),
                ...(phase === 5 || phase === 8
                  ? [
                      {
                        key: 'example',
                        text: 'example publisher: additional details unavailable',
                      },
                    ]
                  : []),
              ],
            },
            theme,
          );
          const job = jobFixture(phase === 1 ? 1 : phase === 2 ? 4 : 3);
          job.command =
            "printf 'Checking presentation fixtures\\n'; npm test -- --reporter=spec; printf 'Check complete\\n'";
          if (phase === 2) job.supervisorExit = {code: null, signal: 'SIGKILL'};
          const goals = new PriorityRow();
          const followed = goalFixtures(
            phase === 9
              ? 2
              : phase === 2
                ? 1
                : phase === 3
                  ? 3
                  : phase === 0
                    ? 11
                    : 8,
          );
          goals.update(
            summaryRow(
              followed,
              followed[0]!.focus.id,
              phase === 0
                ? goalActivityFixture(11, animationFrame())
                : undefined,
            ),
            theme,
          );
          const activity = workers.length
            ? createWidget(() => workers, theme, undefined, animationFrame)
            : undefined;
          inspector.setContent(
            {
              title: 'Application frame',
              detail: 'icon footer · B bottom',
              body: new VStack([
                new UserMessageComponent(
                  'Check the renderer and report any failures.',
                  undefined,
                  0,
                ),
                thinkingComponent(0, false),
                new Spacer(1),
                new Text(
                  theme.fg(
                    'text',
                    'I’ll run the focused checks, then review the result.',
                  ),
                  0,
                  0,
                ),
                new Spacer(1),
                ...transcript(context, phase, expanded, job),
                new Spacer(1),
                ...(context.createEditor
                  ? [context.createEditor(theme), new Spacer(1)]
                  : []),
                goals,
                ...(activity ? [activity] : []),
                jobs,
                ...(combined && phase !== 9
                  ? [
                      createWakeWidget(
                        () => wakeFixtures(0),
                        () => 0,
                        theme,
                      ),
                    ]
                  : []),
                footer,
              ]),
            },
            theme,
          );
        },
      };
    },
  },
  tool('bash', 'Tool output', bashPresentation, [
    {
      label: 'receiving arguments',
      observation: call({command: 'npm te'}, false, false),
    },
    {label: 'queued', observation: call(shell, true, false)},
    {label: 'running', observation: call(shell)},
    {
      label: 'streaming output',
      observation: result(
        shell,
        'Starting tests…\n✓ parser\n',
        undefined,
        false,
        true,
      ),
    },
    {
      label: 'completed',
      observation: result(shell, '✓ parser\n✓ renderer\n2 tests passed'),
    },
    {
      label: 'failed / truncated',
      observation: result(
        shell,
        '✓ parser\n✗ renderer: expected visible diagnostic\nCommand exited with code 1',
        {
          truncation: {truncated: true},
          fullOutputPath: '/fixture/output/test.log',
        },
        true,
      ),
    },
    {
      label: 'long command / resize to reveal',
      observation: result(longCommand, 'Shipping component checks passed'),
    },
    {
      label: 'large output',
      observation: result(
        shell,
        Array.from(
          {length: 1000},
          (_, index) =>
            `output line ${String(index).padStart(4, '0')} 日本語 👩🏽‍💻\n`,
        ).join('') + 'FINAL DIAGNOSTIC: permission denied',
        undefined,
        true,
      ),
    },
  ]),
  tool('read', 'Recorded source', readPresentation, [
    {label: 'source', observation: result({path, offset: 20}, source)},
    {
      label: 'missing file',
      observation: result(
        {path},
        'ENOENT: report.ts is unavailable',
        undefined,
        true,
      ),
    },
    {
      label: 'unknown / mixed content',
      observation: {
        ...result({path}, 'Text accompanying an image'),
        slot: 'result',
        content: [
          {type: 'text', text: 'Text accompanying an image'},
          {type: 'image', data: '', mimeType: 'image/png'},
        ],
        details: undefined,
        partial: false,
        error: false,
        durationMs: 10,
        showImages: true,
      },
    },
  ]),
  tool('write', 'Recorded write', writePresentation, [
    {
      label: 'streaming arguments',
      observation: call({path, content: source.slice(0, 25)}, false, false),
    },
    {
      label: 'completed',
      observation: result({path, content: source}, 'Written'),
    },
  ]),
  tool('edit', 'Recorded diff', editPresentation, [
    {
      label: 'comparison',
      observation: result(editArgs, 'Applied', {
        diff: ' 1 export function greet(name: string) {\n-2   return `Hello, ${name} 👩🏽‍💻`;\n+2   return `Welcome, ${name} 👩🏽‍💻`;\n 3 }',
        patch:
          '@@ -1,3 +1,3 @@\n export function greet(name: string) {\n-  return `Hello, ${name} 👩🏽‍💻`;\n+  return `Welcome, ${name} 👩🏽‍💻`;\n }',
      }),
    },
    {
      label: 'long lines wrap',
      observation: result(editArgs, 'Applied', {
        diff:
          ' 18 export function shippingComponent(label: string) {\n' +
          '-19   return `Package ${label} · variant=classic · preview=compact · width=responsive`;\n' +
          '+19   return `Package ${label} · variant=modern · preview=expanded · width=responsive`;\n' +
          ' 20 }',
        patch:
          '@@ -18,3 +18,3 @@\n export function shippingComponent(label: string) {\n' +
          '-  return `Package ${label} · variant=classic · preview=compact · width=responsive`;\n' +
          '+  return `Package ${label} · variant=modern · preview=expanded · width=responsive`;\n' +
          ' }',
      }),
    },
    {label: 'missing recorded diff', observation: result(editArgs, 'Applied')},
    {
      label: 'conflict',
      observation: result(
        editArgs,
        'Old text did not match uniquely',
        undefined,
        true,
      ),
    },
  ]),
  tool('codemode', 'Nested tool activity', codemodePresentation, [
    {
      label: 'running',
      observation: result(script, '', nested('running'), false, true),
    },
    {
      label: 'caught child failure',
      observation: result(
        script,
        'One check failed; inspect its diagnostic.',
        nested('error'),
      ),
    },
    {
      label: 'nested long path / upstream preview',
      observation: result(script, 'Recorded summaries only', {
        calls: [
          {
            name: 'read',
            args: JSON.stringify({
              path: '/workspace/example/packages/shipping-component/src/日本語/preview.ts',
            }),
            status: 'ok',
          },
          {
            name: 'bash',
            args:
              JSON.stringify({
                command:
                  longCommand.command +
                  ' --checks=unit,integration,terminal --artifact=packages/shipping-component/tests/fixtures/preview.ts',
              }).slice(0, 197) + '...',
            status: 'error',
            error: 'Fixture check failed',
          },
        ],
      }),
    },
    {
      label: 'missing recorded summaries',
      observation: result(script, 'Recorded output without nested details'),
    },
  ]),
  {
    id: 'job',
    title: 'Job inspection',
    phases: [
      'accepted',
      'running',
      'stopping',
      'completed',
      'failed / incomplete capture',
      'unknown history',
    ],
    create(context, theme) {
      let job = jobFixture();
      let previous = -1;
      const view = new JobInspector(
        projectJob(job),
        theme,
        context.bindings,
        context.close,
        context.redraw,
        context.height,
      );
      return {
        component: view,
        canAct: () => canStop(job),
        act: () => (canStop(job) ? 2 : undefined),
        update(phase, _expanded, theme) {
          if (previous !== phase) {
            job = jobFixture(phase);
            previous = phase;
          }
          view.update(projectJob(job), theme);
        },
      };
    },
  },
  {
    id: 'workers',
    title: 'Worker activity',
    phases: [
      'active / asking',
      'quiet / failed call',
      'saved question',
      'one worker',
      'three workers',
      'eight workers',
      'mixed outcomes',
      'long question',
    ],
    create(context) {
      const inspector = new Inspector(
        context.bindings,
        context.close,
        context.redraw,
        context.height,
      );
      return {
        component: inspector,
        update(phase, expanded, theme) {
          const rows: WidgetRow[] = [
            {
              label: 'review',
              model: 'example/reasoning-model',
              thinking: 'high',
              context: 38,
              checkout: {kind: 'branch', label: 'feature/api'},
              headline: 'Check public API',
              state: 'running',
              depth: 0,
              elapsed: '2m',
              calls: 12,
              tokens: 15000,
              detail: 'read src/report.ts',
              idleMs: phase === 1 ? 120000 : 0,
            },
            {
              label: '日本語',
              model: 'example/fast-model',
              thinking: 'medium',
              context: 82,
              checkout: {kind: 'branch', label: 'fix/schema'},
              state: 'asking',
              depth: 1,
              elapsed: '40s',
              calls: 3,
              tokens: 2000,
              detail: 'Which comparison should I use?',
              idleMs: 0,
              ...(phase === 1 ? {failed: 'read'} : {}),
            },
          ];
          if (phase === 3) rows.splice(1);
          if (phase === 4 || phase === 5) {
            rows.splice(
              0,
              rows.length,
              ...Array.from({length: phase === 4 ? 3 : 8}, (_, i) => ({
                label: `review-${i + 1}`,
                model: 'example/reasoning-model',
                thinking: i % 2 ? 'medium' : 'high',
                context: [12, 52, 94][i % 3],
                checkout: {
                  kind: 'branch' as const,
                  label: `feature/review-${i + 1}`,
                },
                headline: 'Check public API',
                state: 'running',
                depth: 0,
                elapsed: '24s',
                calls: 1,
                tokens: 1000,
                detail: 'read src/api/routes.ts',
                idleMs: 0,
              })),
            );
          }
          if (phase === 6)
            rows.push(
              {
                ...rows[0]!,
                label: 'docs',
                state: 'completed',
                finished: true,
                headline: undefined,
                detail: 'Documentation checked',
              },
              {
                ...rows[0]!,
                label: 'tests',
                state: 'failed',
                finished: true,
                headline: undefined,
                detail: 'Provider unavailable',
              },
            );
          if (phase === 7)
            rows[1]!.detail =
              'Waiting for parent: should this API preserve compatibility with the previous release, or may consumers migrate together?';
          const body =
            phase === 2
              ? renderEvent(
                  {
                    role: 'custom',
                    customType: 'dots.worker.event',
                    timestamp: 0,
                    display: true,
                    content:
                      'Worker question\n\nWhich comparison should I use?\nThe saved observation differs from the current checkout.',
                    details: {id: 'w-example', kind: 'asking', label: 'review'},
                  },
                  {
                    expanded,
                    outputPad: 0,
                    ...rendererHints(context.bindings, theme),
                  },
                  theme,
                )
              : createWidget(() => rows, theme, undefined, animationFrame);
          inspector.setContent(
            {title: 'Worker activity', body, hint: ''},
            theme,
          );
        },
      };
    },
  },
  workerInspectorScenario,
  {
    id: 'wake',
    title: 'Scheduled follow-up',
    phases: [
      'delivered',
      'late / failed check',
      'scheduled',
      'checking / queued',
      'many wakes',
      'inspection',
    ],
    create(context) {
      const inspector = new Inspector(
        context.bindings,
        context.close,
        context.redraw,
        context.height,
      );
      return {
        component: inspector,
        update(phase, expanded, theme) {
          const body = renderWake(
            {
              role: 'custom',
              customType: 'dots-wake',
              timestamp: 0,
              display: true,
              content:
                'Scheduled follow-up\n\nInspect the latest test result.\nCheck output: fixture check finished.',
              details: {
                id: 'wake-example',
                fired: 1,
                limit: 3,
                late: phase ? 120000 : 0,
                every: '15m',
                check: 'check-fixture',
                exit: phase ? 1 : 0,
                text: 'Inspect the latest test result',
              },
            },
            {
              expanded,
              outputPad: 0,
              ...rendererHints(context.bindings, theme),
            },
            theme,
          );
          inspector.setContent(
            {
              title: 'Scheduled follow-up',
              body:
                phase < 2
                  ? body
                  : phase === 5
                    ? wakeActivityBody(wakeFixtures(2), 0, theme, true)
                    : createWakeWidget(
                        () => wakeFixtures(phase - 2),
                        () => 0,
                        theme,
                      ),
            },
            theme,
          );
        },
      };
    },
  },
  thinkingScenario,
  goalScenario(),
];
