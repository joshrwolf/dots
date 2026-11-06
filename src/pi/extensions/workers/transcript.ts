import {
  AssistantMessageComponent,
  BashExecutionComponent,
  BranchSummaryMessageComponent,
  CompactionSummaryMessageComponent,
  CustomMessageComponent,
  SkillInvocationMessageComponent,
  ToolExecutionComponent,
  UserMessageComponent,
  createBashToolDefinition,
  createEditToolDefinition,
  createFindToolDefinition,
  createGrepToolDefinition,
  createLsToolDefinition,
  createReadToolDefinition,
  createWriteToolDefinition,
  getMarkdownTheme,
  truncateTail,
  type ExtensionAPI,
  type MessageRenderer,
  type ToolDefinition,
  type ToolRenderers,
} from '@earendil-works/pi-coding-agent';
import {Spacer, type Component, type TUI} from '@earendil-works/pi-tui';
import {OutputView} from '../../src/ui/card.ts';
import {messageRenderers, rendererResolver} from '../ui/index.ts';
import {
  hiddenThinkingLabel,
  thinkingMarkdown,
} from '../../src/features/thinking.ts';
import type {SessionFollower} from './follow.ts';
import {
  projectTranscript,
  type ToolOutcome,
  type TranscriptItem,
} from './projection.ts';
import {fields, type WorkerEvent} from './protocol.ts';

/** Display settings a transcript takes from the host's Pi settings. */
export interface TranscriptOptions {
  readonly outputPad: number;
  readonly hideThinkingBlock: boolean;
  readonly showImages: boolean;
  readonly imageWidthCells: number;
}

export function transcriptOptions(
  settings: ReturnType<ExtensionAPI['getSettings']>,
): TranscriptOptions {
  const width = settings.terminal?.imageWidthCells;
  return {
    outputPad: settings.outputPad === 0 ? 0 : 1,
    hideThinkingBlock: settings.hideThinkingBlock ?? false,
    showImages: settings.terminal?.showImages ?? true,
    imageWidthCells:
      typeof width === 'number' && Number.isFinite(width)
        ? Math.max(1, Math.floor(width))
        : 60,
  };
}

/** The most recent rows a transcript presents; older rows stay in the session file. */
const TRANSCRIPT_ROWS = 200;
/** Live text kept for one streaming response; the saved message replaces it. */
const LIVE_TEXT_LIMIT = 256 * 1024;

type AssistantMessage = ConstructorParameters<
  typeof AssistantMessageComponent
>[0] &
  object;
type LiveBlock = AssistantMessage['content'][number];
type CustomMessage = Parameters<MessageRenderer>[0];

/** The companion's own message renderers, which the worker's Pi registered for these types. */
const customRenderers = messageRenderers();

/** ToolExecutionComponent's own definition type; renderers are typed by their tool's arguments. */
type AnyToolDefinition = ToolDefinition<any, any, any>;
const builtInTools: Readonly<
  Record<string, (cwd: string) => AnyToolDefinition>
> = {
  read: createReadToolDefinition,
  bash: createBashToolDefinition,
  edit: createEditToolDefinition,
  write: createWriteToolDefinition,
  grep: createGrepToolDefinition,
  find: createFindToolDefinition,
  ls: createLsToolDefinition,
};

/** Tool renderers as the worker's own Pi resolves them: the companion's, over Pi's built-in definitions. */
function workerToolRenderers(
  cwd: string,
): (name: string) => ToolRenderers | undefined {
  const resolve = rendererResolver();
  const cache = new Map<string, ToolRenderers | undefined>();
  return name => {
    if (cache.has(name)) return cache.get(name);
    const renderers = resolve(name, () => builtInTools[name]?.(cwd));
    cache.set(name, renderers);
    return renderers;
  };
}

/** A recorded assistant message from an RPC event; undefined for anything else. */
function assistantMessage(value: unknown): AssistantMessage | undefined {
  const message = fields(value);
  return message?.role === 'assistant' && Array.isArray(message.content)
    ? (message as unknown as AssistantMessage)
    : undefined;
}

function toolOutcome(value: unknown): ToolOutcome | undefined {
  const result = fields(value);
  if (!result || !Array.isArray(result.content)) return undefined;
  return {
    content: result.content as ToolOutcome['content'],
    details: result.details,
    isError: false,
  };
}

interface Row {
  readonly component: Component;
  readonly tool?: ToolExecutionComponent;
  /** Whether the row shows its final recorded result. */
  settled: boolean;
}

/**
 * A worker's transcript drawn with Pi's own message components, as an
 * attached Pi would draw the saved session. The follower supplies whole saved
 * messages; a host that receives the worker's RPC events can also pass them to
 * `record`, which shows the response and tool output still in flight.
 * Rendering does no I/O: `refresh` polls, the host decides when.
 */
export class WorkerTranscript implements Component {
  private readonly tui: TUI;
  private readonly follower: SessionFollower;
  private readonly options: TranscriptOptions;
  private readonly rows = new Map<string, Row>();
  private readonly toolKeys = new Map<string, string>();
  private children: Component[] = [];
  private renderers: ((name: string) => ToolRenderers | undefined) | undefined;
  private rendererCwd: string | undefined;
  private expanded = false;
  private live:
    | {
        message: AssistantMessage;
        size: number;
        ended: boolean;
        component: AssistantMessageComponent | undefined;
        dirty: boolean;
      }
    | undefined;
  private readonly started = new Set<string>();
  private readonly partial = new Map<string, ToolOutcome>();

  constructor(tui: TUI, follower: SessionFollower, options: TranscriptOptions) {
    this.tui = tui;
    this.follower = follower;
    this.options = options;
    this.refresh();
  }

  get file(): string | undefined {
    return this.follower.file;
  }

  /** Reads newly saved records; true when the transcript changed. */
  refresh(): boolean {
    const changed = this.follower.poll();
    if (changed || !this.children.length) this.rebuild();
    return changed;
  }

  /** Shows a live RPC event from the worker this transcript follows. */
  record(event: WorkerEvent): void {
    switch (event.type) {
      case 'agent_start':
        this.live = undefined;
        this.started.clear();
        this.partial.clear();
        break;
      case 'agent_settled':
        this.started.clear();
        this.partial.clear();
        break;
      case 'message_start': {
        const message = assistantMessage(event.message);
        if (!message) return;
        this.live = {
          message: {...message, content: [...message.content]},
          size: 0,
          ended: false,
          component: undefined,
          dirty: true,
        };
        break;
      }
      case 'message_update': {
        const live = this.live;
        const update = event.assistantMessageEvent;
        const index = update.contentIndex;
        if (!live || live.ended || index === undefined || index > 1000) return;
        const content = live.message.content as LiveBlock[];
        const block = content[index];
        const room = Math.max(0, LIVE_TEXT_LIMIT - live.size);
        const delta = (update.delta ?? '').slice(0, room);
        switch (update.type) {
          case 'text_start':
            content[index] = {type: 'text', text: ''};
            break;
          case 'text_delta':
            if (block?.type !== 'text') return;
            content[index] = {...block, text: block.text + delta};
            live.size += delta.length;
            break;
          case 'text_end':
            content[index] = {type: 'text', text: update.content ?? ''};
            break;
          case 'thinking_start':
            content[index] = {type: 'thinking', thinking: ''};
            break;
          case 'thinking_delta':
            if (block?.type !== 'thinking') return;
            content[index] = {...block, thinking: block.thinking + delta};
            live.size += delta.length;
            break;
          case 'thinking_end':
            content[index] = {
              type: 'thinking',
              thinking: update.content ?? '',
            };
            break;
          default:
            return;
        }
        live.dirty = true;
        this.tui.requestRender();
        return;
      }
      case 'message_end': {
        const message = assistantMessage(event.message);
        if (!message || !this.live) return;
        this.live.message = message;
        this.live.ended = true;
        this.live.dirty = true;
        break;
      }
      case 'tool_execution_start':
        if (!event.toolCallId || this.started.size > 1000) return;
        this.started.add(event.toolCallId);
        this.pendingTool(event.toolCallId)?.markExecutionStarted();
        return;
      case 'tool_execution_update': {
        const outcome = toolOutcome(event.partialResult);
        if (!outcome || this.partial.size > 1000) return;
        this.partial.set(event.toolCallId, outcome);
        this.pendingTool(event.toolCallId)?.updateResult(outcome, true);
        this.tui.requestRender();
        return;
      }
      case 'tool_execution_end':
        if (event.toolCallId) this.partial.delete(event.toolCallId);
        return;
      default:
        return;
    }
    this.rebuild();
    this.tui.requestRender();
  }

  /** Expands or collapses tool output and summaries, like Pi's tool expansion key. */
  toggleExpanded(): void {
    this.expanded = !this.expanded;
    for (const row of this.rows.values()) {
      const target = row.component as {setExpanded?: (value: boolean) => void};
      target.setExpanded?.(this.expanded);
    }
    this.tui.requestRender();
  }

  /** The shown tool row still waiting for its saved result. */
  private pendingTool(toolCallId: string): ToolExecutionComponent | undefined {
    const row = this.rows.get(this.toolKeys.get(toolCallId) ?? '');
    return row && !row.settled ? row.tool : undefined;
  }

  private toolRenderers(name: string): ToolRenderers | undefined {
    const cwd = this.follower.session.header?.cwd ?? '';
    if (!this.renderers || this.rendererCwd !== cwd) {
      this.renderers = workerToolRenderers(cwd);
      this.rendererCwd = cwd;
    }
    return this.renderers(name);
  }

  private create(item: TranscriptItem): Row {
    const {outputPad} = this.options;
    const markdown = getMarkdownTheme();
    const transformers = [thinkingMarkdown];
    switch (item.kind) {
      case 'user':
        return {
          component: new UserMessageComponent(
            item.text,
            markdown,
            outputPad,
            transformers,
            item.entryId,
            item.timestamp,
          ),
          settled: true,
        };
      case 'skill': {
        const component = new SkillInvocationMessageComponent(
          item.block,
          markdown,
          outputPad,
        );
        component.setExpanded(this.expanded);
        return {component, settled: true};
      }
      case 'assistant':
        return {
          component: new AssistantMessageComponent(
            item.message,
            this.options.hideThinkingBlock,
            markdown,
            hiddenThinkingLabel,
            outputPad,
            transformers,
            item.entryId,
            item.timestamp,
          ),
          settled: true,
        };
      case 'tool': {
        const tool = new ToolExecutionComponent(
          item.call.name,
          item.call.id,
          item.call.arguments,
          {
            showImages: this.options.showImages,
            imageWidthCells: this.options.imageWidthCells,
            outputPad,
          },
          this.toolRenderers(item.call.name),
          this.tui,
          this.follower.session.header?.cwd ?? '',
        );
        tool.setExpanded(this.expanded);
        return {component: tool, tool, settled: false};
      }
      case 'bash': {
        const bash = new BashExecutionComponent(
          item.message.command,
          this.tui,
          item.message.excludeFromContext,
          outputPad,
        );
        if (item.message.output) bash.appendOutput(item.message.output);
        bash.setComplete(
          item.message.exitCode,
          item.message.cancelled,
          // The record keeps only whether its output was truncated when saved.
          item.message.truncated
            ? {...truncateTail(item.message.output), truncated: true}
            : undefined,
          item.message.fullOutputPath,
        );
        bash.setExpanded(this.expanded);
        return {component: bash, settled: true};
      }
      case 'custom': {
        const component = new CustomMessageComponent(
          item.message as CustomMessage,
          customRenderers.get(item.message.customType),
          markdown,
          outputPad,
        );
        component.setExpanded(this.expanded);
        return {component, settled: true};
      }
      case 'compaction': {
        const component = new CompactionSummaryMessageComponent(
          item.message,
          markdown,
          outputPad,
        );
        component.setExpanded(this.expanded);
        return {component, settled: true};
      }
      case 'branch': {
        const component = new BranchSummaryMessageComponent(
          item.message,
          markdown,
          outputPad,
        );
        component.setExpanded(this.expanded);
        return {component, settled: true};
      }
    }
  }

  /** Brings row components in line with the saved items and the live response. */
  private rebuild(): void {
    const session = this.follower.session;
    const items = projectTranscript(session.entries);
    const shown = items.slice(-TRANSCRIPT_ROWS);
    this.toolKeys.clear();
    for (const item of shown)
      if (item.kind === 'tool') this.toolKeys.set(item.call.id, item.key);
    const keep = new Set(shown.map(item => item.key));
    for (const key of this.rows.keys())
      if (!keep.has(key)) this.rows.delete(key);
    const children: Component[] = [];
    if (session.notice) children.push(new OutputView(`(${session.notice})`));
    const omitted = items.length - shown.length;
    if (omitted > 0 || session.truncated)
      children.push(
        new OutputView(
          `${session.truncated ? 'Earlier rows' : `${omitted} earlier row${omitted === 1 ? '' : 's'}`} not shown; full history: ${this.follower.file ?? ''}`,
        ),
      );
    let persisted = false;
    for (const item of shown) {
      let row = this.rows.get(item.key);
      if (!row) {
        row = this.create(item);
        this.rows.set(item.key, row);
        // A saved call's arguments are final; only live events say it runs.
        if (item.kind === 'tool' && !item.result && row.tool) {
          row.tool.setArgsComplete();
          if (this.started.has(item.call.id)) row.tool.markExecutionStarted();
          const partial = this.partial.get(item.call.id);
          if (partial) row.tool.updateResult(partial, true);
        }
      }
      if (item.kind === 'tool' && item.result && row.tool && !row.settled) {
        row.tool.updateResult(item.result);
        row.settled = true;
      }
      if (
        item.kind === 'assistant' &&
        this.live &&
        item.message.timestamp === this.live.message.timestamp
      )
        persisted = true;
      if (
        item.kind === 'user' ||
        item.kind === 'skill' ||
        item.kind === 'compaction' ||
        item.kind === 'branch'
      )
        children.push(new Spacer(1));
      children.push(row.component);
    }
    if (persisted) this.live = undefined;
    if (this.live) {
      this.live.component ??= new AssistantMessageComponent(
        undefined,
        this.options.hideThinkingBlock,
        getMarkdownTheme(),
        hiddenThinkingLabel,
        this.options.outputPad,
        [thinkingMarkdown],
      );
      children.push(this.live.component);
    }
    if (!shown.length && !this.live && !session.notice)
      children.push(new OutputView('No recorded messages yet.'));
    this.children = children;
  }

  invalidate(): void {
    for (const child of this.children) child.invalidate();
  }

  render(width: number): string[] {
    if (width <= 0) return [];
    const live = this.live;
    // Applying at most one streaming update per frame bounds Markdown work to
    // the host's render rate rather than the provider's delta rate.
    if (live?.component && live.dirty) {
      live.dirty = false;
      live.component.updateContent(live.message, !live.ended);
    }
    return this.children.flatMap(child => child.render(width));
  }

  dispose(): void {
    this.live = undefined;
    this.rows.clear();
    this.toolKeys.clear();
    this.children = [];
    this.started.clear();
    this.partial.clear();
  }
}
