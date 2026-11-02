import type {Theme} from '@earendil-works/pi-coding-agent';
import {VStack, type Component} from '@earendil-works/pi-tui';
import type {
  PresentationOptions,
  PresentationView,
  ToolObservation,
  ToolPresentation,
} from '../../host/tools.ts';
import {
  activityHeader,
  Notices,
  OutputView,
  PreviewView,
  ActivityCard,
  type Notice,
} from '../../ui/card.ts';
import {
  displayLimits,
  PriorityRow,
  plainLines,
  fitLabel,
  labelText,
} from '@joshrwolf/pi-kit/ui';
import {StateGutter} from '../../ui/state.ts';
import {DetailView} from '../../ui/details.ts';
import {SourceView} from '../../ui/previews/view.ts';
import type {SyntaxService} from '../../ui/previews/syntax.ts';
import {
  activityState,
  duration,
  outcome,
  record,
  pathLabel,
  segment,
  string,
  type Outcome,
} from './records.ts';

const CALL_LIMIT = 256;
const statuses = ['running', 'ok', 'error', 'cancelled'] as const;
type Status = (typeof statuses)[number];
interface Call {
  readonly name: string;
  readonly args: string;
  readonly status: Status;
  readonly error: string;
  readonly durationMs?: number;
  readonly cost?: number;
}
interface CodemodeModel {
  readonly code: string;
  readonly cwd: string | undefined;
  readonly calls: readonly Call[];
  readonly total: number;
  readonly available: boolean;
  readonly unreadable: number;
  readonly fullOutput: string;
  readonly output: string;
  readonly outcome: Outcome;
}
const finite = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value) && value >= 0;
const bounded = (value: string, limit = 2000): string =>
  value.length <= limit ? value : `${value.slice(0, limit)}… [display limited]`;

/** Only the codemode result's recorded summaries; never parse abbreviated JSON as real arguments. */
export function projectCodemode(frame: ToolObservation): CodemodeModel {
  const details = frame.slot === 'result' ? record(frame.details) : undefined;
  const raw: unknown[] | undefined = Array.isArray(details?.calls)
    ? details.calls
    : undefined;
  const calls: Call[] = [];
  let unreadable = 0;
  for (const value of (raw ?? []).slice(0, CALL_LIMIT)) {
    const call = record(value);
    const status = statuses.find(status => status === call?.status);
    if (
      !call ||
      typeof call.name !== 'string' ||
      typeof call.args !== 'string' ||
      !status
    ) {
      unreadable++;
      continue;
    }
    calls.push({
      name: bounded(call.name, 200),
      args: bounded(call.args),
      status,
      error: bounded(string(call.error) ?? ''),
      ...(finite(call.durationMs) ? {durationMs: call.durationMs} : {}),
      ...(finite(call.cost) ? {cost: call.cost} : {}),
    });
  }
  // Remove only the exact native header block, not similarly-worded user output.
  const header =
    /^Script (completed|failed)\nWall time [\d.]+ seconds\nOutput:\n$/;
  const content = frame.slot === 'result' ? frame.content : [];
  const first = content[0];
  const output = outcome(
    frame.slot === 'result'
      ? {
          ...frame,
          content:
            first?.type === 'text' && header.test(first.text)
              ? content.slice(1)
              : content,
        }
      : frame,
  );
  return {
    code: string(record(frame.args)?.code) ?? '',
    cwd: frame.cwd,
    calls,
    total: raw?.length ?? 0,
    available: raw !== undefined,
    unreadable,
    fullOutput: bounded(string(details?.fullOutputPath) ?? ''),
    output: output.kind === 'result' ? output.text : '',
    outcome: outcome(frame),
  };
}

/** Only complete recorded string tokens become subjects; abbreviated JSON stays in details. */
function callLabel(call: Call, cwd: string | undefined): string {
  const field = ['read', 'write', 'edit'].includes(call.name)
    ? 'path'
    : call.name === 'bash' || call.name === 'job_start'
      ? 'command'
      : undefined;
  if (!field) return call.name;
  const match = /^\s*\{\s*"(path|command)"\s*:\s*("(?:[^"\\]|\\.)*")/.exec(
    call.args,
  );
  if (match?.[1] !== field || !match[2]) return call.name;
  try {
    const value: unknown = JSON.parse(match[2]);
    return typeof value === 'string'
      ? field === 'path'
        ? pathLabel(value, cwd)
        : value
      : call.name;
  } catch {
    return call.name;
  }
}

function sharedPathPrefix(
  calls: readonly Call[],
  cwd: string | undefined,
): string {
  const paths = [
    ...new Set(
      calls
        .filter(call => ['read', 'write', 'edit'].includes(call.name))
        .map(call => callLabel(call, cwd))
        .filter(path => path.startsWith('/') || path.startsWith('~/')),
    ),
  ];
  if (paths.length < 2) return '';
  const directories = paths.map(path => path.split('/').slice(0, -1));
  const common = directories[0]!;
  let length = common.length;
  for (const directory of directories.slice(1)) {
    let index = 0;
    while (index < length && common[index] === directory[index]) index++;
    length = index;
  }
  // Keep the last shared directory as an anchor; never shorten at a partial component.
  return length > 2 ? common.slice(0, length - 1).join('/') + '/' : '';
}

function compactLabel(
  call: Call,
  cwd: string | undefined,
  prefix: string,
): string {
  let text = callLabel(call, cwd);
  const path = ['read', 'write', 'edit'].includes(call.name);
  if (path && prefix && text.startsWith(prefix))
    text = `…/${text.slice(prefix.length)}`;
  return fitLabel(labelText(text), 72, path ? 'middle' : 'end');
}

const label = (status: Status): string =>
  status === 'ok' ? 'done' : status === 'error' ? 'failed' : status;

/** Expanded history preserves input order; live receipts show only active calls. */
class CallList implements Component {
  private readonly children: Component[] = [];
  private readonly expanded: boolean;
  constructor(
    calls: readonly Call[],
    cwd: string | undefined,
    theme: Theme,
    expanded: boolean,
  ) {
    this.expanded = expanded;
    const prefix = sharedPathPrefix(calls, cwd);
    const selected = expanded
      ? calls
      : calls.filter(call => call.status === 'running').slice(0, 2);
    for (const call of selected) {
      const row = new PriorityRow();
      row.update(
        {
          layout: 'inline',
          separator: '  ',
          groupSeparator: '  ',
          endSeparator: ' ',
          start: [
            {...segment(call.name, 'muted', 90), padTo: 5},
            segment(
              expanded ? call.args : compactLabel(call, cwd, prefix),
              'dim',
              10,
            ),
          ],
          end: [
            ...(call.durationMs === undefined ||
            (!expanded && call.durationMs < 1000)
              ? []
              : [
                  {
                    ...segment(
                      `${(call.durationMs / 1000).toFixed(1)}s`,
                      'dim',
                      0,
                    ),
                    separatorBefore: ' ',
                    shrink: 'omit' as const,
                  },
                ]),
            ...(call.cost === undefined
              ? []
              : [
                  {
                    ...segment(`$${call.cost.toPrecision(3)}`, 'dim', 0),
                    shrink: 'omit' as const,
                  },
                ]),
          ],
        },
        theme,
      );
      this.children.push(
        new StateGutter(
          row,
          call.status === 'ok'
            ? 'done'
            : call.status === 'error'
              ? 'failed'
              : call.status,
          theme,
        ),
      );
      if (expanded) {
        const notice = new Notices();
        notice.update(
          [
            {
              text: `${call.name} · ${label(call.status)}${call.durationMs === undefined ? '' : ` · ${(call.durationMs / 1000).toFixed(1)}s`}${call.cost === undefined ? '' : ` · $${call.cost.toPrecision(3)}`}`,
              severity: 'dim',
            },
            ...(call.args
              ? [
                  {
                    text: `Arguments (${call.args.length === 200 && call.args.endsWith('...') ? 'upstream-truncated preview' : 'preview'}): ${call.args}`,
                    severity: 'dim' as const,
                  },
                ]
              : []),
            ...(call.error
              ? [{text: call.error, severity: 'error' as const}]
              : []),
          ],
          theme,
        );
        this.children.push(notice);
      }
    }
  }
  invalidate(): void {
    for (const child of this.children) child.invalidate();
  }
  render(width: number): string[] {
    if (width <= 0) return [];
    const lines: string[] = [];
    const budget = this.expanded ? displayLimits.previewRows : 5;
    for (const child of this.children) {
      lines.push(...child.render(width));
      if (lines.length >= budget)
        return [
          ...lines.slice(0, budget - 1),
          ...plainLines('… call display limited', width, 1),
        ];
    }
    return lines;
  }
}

class CodemodeView implements PresentationView<CodemodeModel> {
  private readonly card = new ActivityCard();
  private readonly content = new VStack();
  private readonly syntax: SyntaxService;
  private readonly notify: () => void;
  private code = '';
  private preview: SourceView | undefined;
  constructor(syntax: SyntaxService, notify: () => void) {
    this.syntax = syntax;
    this.notify = notify;
  }
  update(
    model: CodemodeModel,
    theme: Theme,
    options: PresentationOptions,
  ): void {
    const {expanded} = options;
    const state = model.outcome;
    const notices: Notice[] = [];
    if (state.kind === 'result' && !model.available)
      notices.push({
        text: 'Nested call summaries unavailable in this result.',
        severity: 'dim',
      });
    if (model.unreadable)
      notices.push({
        text: `${model.unreadable} call records could not be displayed.`,
        severity: 'warning',
      });
    if (model.total > CALL_LIMIT)
      notices.push({
        text: `Call display limited to the first ${CALL_LIMIT} of ${model.total} records; counts below cover this sample only.`,
        severity: 'warning',
      });
    if (model.fullOutput)
      notices.push({
        text: `Full output: ${model.fullOutput}`,
        severity: 'warning',
      });
    const failed = model.calls.filter(call => call.status === 'error');
    if (!expanded && failed.length > 2)
      notices.push({
        text: `${failed.length - 2} more failed calls; expand to inspect.`,
        severity: 'error',
      });
    if (model.code.length > displayLimits.sourceCodeUnits)
      notices.push({
        text: 'Script display limited; recorded arguments retain the complete script.',
        severity: 'warning',
      });
    const body = new VStack();
    const counts = statuses
      .map(
        status =>
          [
            status,
            model.calls.filter(call => call.status === status).length,
          ] as const,
      )
      .filter(([status, count]) => count && (expanded || status !== 'ok'))
      .map(([status, count]) => `${count} ${label(status)}`)
      .join(' · ');
    const priced = model.calls.filter(call => call.cost !== undefined);
    const cost = priced.reduce((total, call) => total + (call.cost ?? 0), 0);
    const summaryText = [
      counts || (model.available && model.total === 0 ? '0 calls' : ''),
      ...(priced.length
        ? [
            Number.isFinite(cost)
              ? `reported cost $${cost.toPrecision(3)}`
              : 'reported cost exceeds display range',
          ]
        : []),
    ]
      .filter(Boolean)
      .join(' · ');
    if (expanded && model.calls.length)
      body.addChild(new CallList(model.calls, model.cwd, theme, true));
    if (expanded && model.available) {
      const notice = new Notices();
      notice.update(
        [
          {
            text: 'Recorded summaries only: arguments/errors may be abbreviated; nested results are not recorded.',
            severity: 'dim',
          },
        ],
        theme,
      );
      body.addChild(notice);
    }
    if (model.code !== this.code) {
      this.code = model.code;
      this.preview = undefined;
    }
    if (expanded && model.code) {
      this.preview ??= new SourceView({
        source: model.code.slice(0, displayLimits.sourceCodeUnits),
        path: 'script.js',
        theme,
        syntax: this.syntax,
        invalidate: this.notify,
      });
      this.preview.setTheme(theme);
      body.addChild(this.preview);
    }
    if (expanded && model.output) body.addChild(new OutputView(model.output));
    const mix = new Map<string, number>();
    for (const call of model.calls)
      mix.set(call.name, (mix.get(call.name) ?? 0) + 1);
    const toolMix = [...mix]
      .map(([name, count]) =>
        count === 1
          ? name
          : ['read', 'write', 'edit'].includes(name)
            ? `${count} ${name}s`
            : `${count} ${name} calls`,
      )
      .join(' · ');
    this.card.update(
      {
        expanded,
        ...(state.kind === 'result' ? {state: activityState(state)} : {}),
        header: activityHeader('codemode', toolMix, [
          ...(state.kind === 'result' && state.error
            ? [segment('script failed', 'error', 110)]
            : []),
          ...(summaryText
            ? [segment(summaryText, failed.length ? 'warning' : 'dim', 100)]
            : []),
          ...duration(state, expanded),
        ]),
        notices: expanded ? notices : [],
        body:
          expanded &&
          (model.calls.length || model.output || model.available || model.code)
            ? body
            : undefined,
        caption: expanded ? 'calls / recorded script / output' : '',
      },
      theme,
    );
    this.content.clear();
    this.content.addChild(this.card);
    if (!expanded) {
      const details = new VStack();
      const prefix = sharedPathPrefix(model.calls, model.cwd);
      const diagnostics = failed.slice(0, 2).map(call => ({
        name: call.name,
        text: call.error || 'failed (no error text recorded)',
      }));
      if (state.kind === 'result' && state.error)
        diagnostics.push({
          name: 'script',
          text: model.output || 'No diagnostic recorded.',
        });
      if (
        state.kind === 'result' &&
        state.partial &&
        model.calls.some(call => call.status === 'running')
      ) {
        details.addChild(new CallList(model.calls, model.cwd, theme, false));
      } else if (model.calls.length) {
        const subjects = [
          ...new Map(
            model.calls.map(call => [callLabel(call, model.cwd), call]),
          ).values(),
        ];
        const row = new PriorityRow();
        row.update(
          {
            layout: 'inline',
            separator: ' · ',
            start: subjects.slice(0, 3).map(subject => ({
              ...segment(compactLabel(subject, model.cwd, prefix), 'dim', 10),
            })),
            end: [
              ...(subjects.length > 3
                ? [segment(`+${subjects.length - 3} more`, 'dim', 50)]
                : []),
            ],
            groupSeparator: ' · ',
            endSeparator: ' · ',
          },
          theme,
        );
        details.addChild(row);
      }
      for (const diagnostic of diagnostics) {
        const tail = diagnostic.text.slice(-600).trimEnd().split('\n');
        const preview = (tail.length > 1 ? '… ' : '') + (tail.at(-1) ?? '');
        const row = new PriorityRow();
        row.update(
          {
            layout: 'inline',
            start: [
              segment(`↳ ${diagnostic.name}:`, 'error', 90),
              segment(labelText(preview), 'error', 40),
            ],
            end: [],
          },
          theme,
        );
        details.addChild(row);
      }
      if (
        model.output &&
        !(state.kind === 'result' && state.error) &&
        (!model.calls.length ||
          !model.available ||
          model.unreadable ||
          model.calls.some(
            call =>
              ![
                'read',
                'write',
                'edit',
                'bash',
                'job_start',
                'job_status',
                'wake',
                'wake_cancel',
              ].includes(call.name),
          ))
      ) {
        details.addChild(new PreviewView(model.output, theme, options, 2));
      }
      if (notices.length) {
        const warnings = new Notices();
        warnings.update(notices, theme);
        details.addChild(warnings);
      }
      if (
        model.calls.length ||
        model.output ||
        notices.length ||
        diagnostics.length
      )
        this.content.addChild(new DetailView(details, '', theme));
    }
  }
  invalidate(): void {
    this.content.invalidate();
  }
  render(width: number): string[] {
    return this.content.render(width);
  }
}
export function codemodePresentation(
  syntax: SyntaxService,
): ToolPresentation<CodemodeModel> {
  return {
    shell: 'self',
    project: projectCodemode,
    create: notify => new CodemodeView(syntax, notify),
  };
}
