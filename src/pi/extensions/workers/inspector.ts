import {
  getSelectListTheme,
  type KeybindingsManager,
  type Theme,
} from '@earendil-works/pi-coding-agent';
import {
  Input,
  SelectList,
  Text,
  VStack,
  matchesKey,
  truncateToWidth,
  type Component,
  type Focusable,
} from '@earendil-works/pi-tui';
import {Panel, type RowModel} from '@joshrwolf/pi-kit/ui';
import {Inspector} from '../../src/ui/inspector.ts';
import {MarkdownView} from '../../src/ui/markdown.ts';
import {OutputView} from '../../src/ui/card.ts';
import {labelText} from '../../src/index.ts';
import type {TranscriptWindow} from './tail.ts';

export interface WorkerListItem {
  id: string;
  label: string;
  state: string;
  depth: number;
  owner?: string;
  canMessage: boolean;
  canStop: boolean;
}
export interface WorkerSnapshot extends TranscriptWindow {
  worker: WorkerListItem;
  detail: string;
  sessionFile?: string;
}
export interface WorkerInspectionSource {
  list(): WorkerListItem[];
  notice?(): string | undefined;
  read(id: string): WorkerSnapshot | undefined;
  message(id: string, text: string): Promise<void>;
  stop(id: string): Promise<void>;
}

/** Prose is Markdown; tool output/arguments stay literal. Never render raw terminal escapes. */
export function messageText(
  value: unknown,
): {role: string; text: string; markdown: boolean} | undefined {
  if (!value || typeof value !== 'object') return;
  const message = value as Record<string, unknown>;
  const role =
    typeof message.role === 'string' ? labelText(message.role) : 'message';
  const blocks = Array.isArray(message.content)
    ? message.content
    : typeof message.content === 'string'
      ? [{type: 'text', text: message.content}]
      : [];
  const parts: string[] = [];
  let length = 0;
  for (const block of blocks) {
    if (!block || typeof block !== 'object') continue;
    const b = block as Record<string, unknown>;
    const text =
      b.type === 'text' && typeof b.text === 'string'
        ? b.text
        : b.type === 'toolCall'
          ? `→ ${labelText(String(b.name ?? 'tool'))} ${JSON.stringify(b.arguments ?? {})}`
          : b.type === 'image'
            ? '[image; inspect the saved session]'
            : '';
    if (!text) continue;
    parts.push(text.slice(0, Math.max(0, 24_000 - length)));
    length += text.length;
    if (length > 24_000) {
      parts.push('[message display limited; inspect the saved session]');
      break;
    }
  }
  if (!parts.length) return;
  return {
    role:
      role === 'toolResult'
        ? `tool ${labelText(String(message.toolName ?? 'result'))}`
        : role,
    text: parts.join('\n'),
    markdown:
      role === 'assistant' &&
      !blocks.some(
        (b: unknown) =>
          b &&
          typeof b === 'object' &&
          (b as {type?: string}).type === 'toolCall',
      ),
  };
}

/** Why a worker is not controllable here: a descendant of another owner, or history inherited by a fork or clone. */
function readOnly(worker: WorkerListItem): string {
  if (!worker.owner) return '';
  return worker.depth === 0
    ? `Read-only history; owned by ${labelText(worker.owner)}, which this session was forked or cloned from. Spawn a new worker to continue.\n`
    : `Read-only descendant; message owner ${labelText(worker.owner)} to steer or stop.\n`;
}

/** One bounded interactive observer. Reads happen on refresh, never during rendering.
 * Source actions revalidate ownership; displayed controls are not authority. */
export class WorkerInspector extends VStack implements Focusable {
  private readonly source: WorkerInspectionSource;
  private readonly theme: Theme;
  private readonly bindings: Pick<KeybindingsManager, 'matches'>;
  private readonly close: () => void;
  private readonly redraw: () => void;
  private readonly height: () => number;
  private readonly transcript: Inspector;
  private readonly pickerPanel = new Panel();
  private transcriptBody: Component = new Text('', 0, 0);
  private frameKey = '';
  private pickerRows = 0;
  private readonly input = new Input({prompt: 'Message: '});
  private picker: SelectList | undefined;
  private rosterKey = '';
  private items: WorkerListItem[] = [];
  private selected: string | undefined;
  private snapshot: WorkerSnapshot | undefined;
  private stream: ReturnType<typeof messageText>;
  private readonly streamParts = new Map<number, string>();
  private bodyKey = '';
  private composing = false;
  private confirming = false;
  private busy = false;
  private closed = false;
  private status = '';
  private hasFocus = false;
  private streamTimer: ReturnType<typeof setTimeout> | undefined;

  constructor(
    source: WorkerInspectionSource,
    theme: Theme,
    bindings: Pick<KeybindingsManager, 'matches'>,
    close: () => void,
    redraw: () => void,
    height: () => number,
    id?: string,
  ) {
    super();
    this.source = source;
    this.theme = theme;
    this.bindings = bindings;
    this.close = close;
    this.redraw = redraw;
    this.height = height;
    this.transcript = new Inspector(bindings, close, redraw, height, 'end');
    this.input.onEscape = () => {
      this.composing = false;
      this.mount();
      this.redraw();
    };
    this.input.onSubmit = text => {
      if (!text.trim()) {
        this.composing = false;
        this.mount();
        this.redraw();
        return;
      }
      this.start('message', text);
    };
    this.selected = id;
    this.refresh();
  }

  get focused(): boolean {
    return this.hasFocus;
  }
  set focused(value: boolean) {
    this.hasFocus = value;
    this.input.focused = value && this.composing;
  }
  get selectedId(): string | undefined {
    return this.selected;
  }
  get isFollowingEnd(): boolean {
    return this.transcript.isFollowingEnd;
  }

  refresh(): void {
    if (this.closed) return;
    try {
      if (this.selected) {
        this.snapshot = this.source.read(this.selected);
        if (!this.snapshot) {
          this.composing = false;
          this.confirming = false;
        }
        this.updateTranscript();
      } else {
        const all = this.source.list();
        this.items = all.slice(0, 200);
        const key = JSON.stringify(this.items);
        if (key !== this.rosterKey) {
          this.rosterKey = key;
          this.pickerRows = 0;
        }
        this.updatePicker();
        this.status = [
          all.length > 200 ? 'First 200 workers shown' : '',
          this.source.notice?.(),
        ]
          .filter(Boolean)
          .join(' · ');
      }
    } catch (error) {
      this.snapshot = undefined;
      this.composing = false;
      this.confirming = false;
      this.status = `Inspection unavailable: ${labelText(String(error)).slice(0, 256)}`;
      if (this.selected) this.updateTranscript();
    }
    this.mount();
    this.redraw();
  }

  private select(id: string): void {
    this.selected = id;
    this.snapshot = undefined;
    this.stream = undefined;
    this.streamParts.clear();
    if (this.streamTimer) clearTimeout(this.streamTimer);
    this.streamTimer = undefined;
    this.bodyKey = '';
    this.status = '';
    this.confirming = false;
    this.transcript.followEnd();
    this.refresh();
  }

  /** RPC exports text deltas, not partial-message snapshots. Keep only 24 KiB
   * of the current response; saved JSONL supplies the bounded recent log. */
  record(
    id: string,
    record: {type: string; message?: unknown; assistantMessageEvent?: unknown},
  ): void {
    if (this.closed || id !== this.selected) return;
    if (record.type === 'message_update') {
      const update = record.assistantMessageEvent as
        | {
            type?: string;
            contentIndex?: number;
            delta?: string;
            content?: string;
          }
        | undefined;
      const index = update?.contentIndex;
      if (typeof index !== 'number' || index < 0 || index > 100) return;
      if (update?.type === 'text_start') this.streamParts.set(index, '');
      else if (
        update?.type === 'text_delta' &&
        typeof update.delta === 'string'
      ) {
        const used = [...this.streamParts.values()].reduce(
          (total, part) => total + part.length,
          0,
        );
        const room = Math.max(0, 24_000 - used);
        this.streamParts.set(
          index,
          (this.streamParts.get(index) ?? '') + update.delta.slice(0, room),
        );
      } else if (update?.type !== 'text_end') return;
      const text = [...this.streamParts.entries()]
        .sort(([a], [b]) => a - b)
        .map(([, part]) => part)
        .join('\n');
      this.stream = {
        role: 'assistant',
        text: `${text}${text.length >= 24_000 ? '\n[stream display limited; inspect the saved session]' : ''}`,
        markdown: true,
      };
      if (!this.streamTimer)
        this.streamTimer = setTimeout(() => {
          this.streamTimer = undefined;
          if (!this.closed) {
            this.updateTranscript();
            this.redraw();
          }
        }, 120);
    } else if (
      record.type === 'message_end' ||
      record.type === 'agent_start' ||
      record.type === 'agent_settled'
    ) {
      if (this.streamTimer) clearTimeout(this.streamTimer);
      this.streamTimer = undefined;
      if (record.type === 'agent_start') {
        this.streamParts.clear();
        this.stream = undefined;
      }
      if (record.type === 'message_end') {
        this.streamParts.clear();
        const final = messageText(record.message);
        if (final?.role === 'assistant') this.stream = final;
      }
      this.refresh();
    }
  }

  private updateTranscript(): void {
    const snapshot = this.snapshot;
    // Bound both retained projection and preparation. This is a recent log, not
    // a reconstruction of the child's active branch after forks/compaction.
    const messages =
      snapshot?.messages
        .slice(-40)
        .map(messageText)
        .filter(m => m !== undefined) ?? [];
    const latest = messages.at(-1);
    const live =
      this.stream &&
      !(latest?.role === this.stream.role && latest.text === this.stream.text)
        ? this.stream
        : undefined;
    const key = JSON.stringify([
      snapshot?.detail,
      snapshot?.notice,
      snapshot?.worker,
      snapshot?.sessionFile,
      messages,
      live,
    ]);
    if (key === this.bodyKey) return;
    this.bodyKey = key;
    const body = new VStack();
    if (!snapshot)
      body.addChild(
        new OutputView('Worker unavailable. Return to the list to refresh.'),
      );
    else {
      body.addChild(
        new OutputView(
          `${snapshot.detail}\n${readOnly(snapshot.worker)}Recent recorded messages · full history: ${snapshot.sessionFile ?? 'session not saved yet'}`,
        ),
      );
      if (snapshot.notice || snapshot.messages.length > 40)
        body.addChild(
          new OutputView(
            snapshot.notice ?? 'Older messages omitted from this view.',
          ),
        );
      for (const message of [
        ...messages,
        ...(live ? [{...live, role: `${live.role} · live/last response`}] : []),
      ]) {
        body.addChild(
          new Text(this.theme.fg('muted', `\n${message.role}`), 0, 0),
        );
        body.addChild(
          message.markdown
            ? new MarkdownView(message.text, this.theme, 400)
            : new OutputView(message.text),
        );
      }
      if (!messages.length && !live)
        body.addChild(
          new OutputView(
            snapshot.messages.length
              ? 'No displayable text in recent recorded messages.'
              : 'No recorded messages yet.',
          ),
        );
    }
    this.transcriptBody = body;
    this.frameKey = '';
  }

  private updatePicker(): void {
    // SelectList has no public max-visible setter. Recreate on resize, preserving
    // selection; leave its navigation and mouse handling entirely native.
    const rows = Math.max(1, this.height() - 1); // Allow its overflow indicator.
    if (this.picker && this.pickerRows === rows) return;
    const selected = this.picker?.getSelectedItem()?.value;
    this.pickerRows = rows;
    this.picker = new SelectList(
      this.items.map(item => ({
        value: item.id,
        label: `${'  '.repeat(Math.min(item.depth, 8))}${item.depth ? '↳ ' : ''}${labelText(item.label)}`,
        description: `${labelText(item.state)} · ${item.owner ? `read-only · owner ${labelText(item.owner)}` : 'owned'} · ${labelText(item.id)}`,
      })),
      rows,
      getSelectListTheme(),
    );
    if (selected)
      this.picker.setSelectedIndex(
        Math.max(
          0,
          this.items.findIndex(item => item.id === selected),
        ),
      );
    this.picker.onSelect = item => this.select(item.value);
    this.picker.onCancel = this.close;
    this.frameKey = '';
  }

  private footer(hint: string): RowModel {
    const notice = labelText(this.status);
    return {
      layout: 'inline',
      groupSeparator: '  ',
      start: notice
        ? [{text: notice, tone: 'warning', priority: 100, shrink: 'clip'}]
        : [{text: hint, tone: 'dim', priority: 100, shrink: 'clip'}],
      end: notice
        ? [{text: hint, tone: 'dim', priority: 10, shrink: 'omit'}]
        : [],
    };
  }

  private syncShell(): void {
    if (!this.selected) this.updatePicker();
    const worker = this.snapshot?.worker;
    const hint = this.busy
      ? 'Sending action…'
      : this.composing
        ? 'Enter send · Esc cancel message'
        : this.confirming
          ? 'Stop this worker and its descendants? Press X again to confirm · any other key cancels'
          : this.selected
            ? `${this.isFollowingEnd ? 'Following' : 'Paused'} · ↑/↓ scroll · F follow${worker?.canMessage ? ' · M message' : ''}${worker?.canStop ? ' · X stop' : ''} · L list · Esc close`
            : '↑/↓ select · Enter inspect · Esc close';
    const key = JSON.stringify([
      this.selected,
      this.bodyKey,
      this.rosterKey,
      this.pickerRows,
      this.composing,
      hint,
      this.status,
    ]);
    if (key === this.frameKey) return;
    this.frameKey = key;
    if (this.selected)
      this.transcript.setContent(
        {
          title: `Worker · ${labelText(worker?.label ?? this.selected)}`,
          detail: worker ? labelText(worker.state) : 'unavailable',
          body: this.transcriptBody,
          ...(this.composing ? {accessory: this.input} : {}),
          footer: this.footer(hint),
        },
        this.theme,
      );
    else
      this.pickerPanel.update(
        {
          heading: {
            start: [
              {
                text: 'Workers',
                tone: 'toolTitle',
                bold: true,
                priority: 100,
                shrink: 'clip',
              },
            ],
            end: [
              {
                text: `${this.items.length} retained`,
                tone: 'muted',
                priority: 10,
                shrink: 'omit',
              },
            ],
          },
          body:
            this.items.length && this.picker
              ? this.picker
              : new Text('No retained workers.', 0, 0),
          footer: this.footer(hint),
        },
        this.theme,
      );
  }

  private mount(): void {
    this.clear();
    this.addChild(this.selected ? this.transcript : this.pickerPanel);
    this.input.focused = this.hasFocus && this.composing;
    this.frameKey = '';
  }

  override render(width: number): string[] {
    if (this.closed || width <= 0) return [];
    this.syncShell();
    // SelectList's narrow-width fallback can exceed a single column. Bound the
    // final composed overlay without replacing its native selection or layout.
    return super.render(width).map(line => truncateToWidth(line, width, ''));
  }

  handleInput(data: string): void {
    if (this.closed) return;
    if (this.composing && !this.busy) {
      this.input.handleInput(data);
      this.redraw();
      return;
    }
    if (this.bindings.matches(data, 'tui.select.cancel')) {
      this.close();
      return;
    }
    if (this.busy) return;
    if (!this.selected) {
      this.picker?.handleInput(data);
      this.redraw();
      return;
    }
    const pressed = (key: 'f' | 'l' | 'm' | 'x') =>
      matchesKey(data, key) || matchesKey(data, `shift+${key}`);
    if (this.confirming) {
      this.confirming = false;
      if (pressed('x')) {
        this.start('stop');
        return;
      }
      this.redraw();
      return;
    }
    if (pressed('l')) {
      this.selected = undefined;
      this.stream = undefined;
      this.streamParts.clear();
      this.status = '';
      if (this.streamTimer) clearTimeout(this.streamTimer);
      this.streamTimer = undefined;
      this.refresh();
      return;
    }
    if (pressed('f')) {
      this.transcript.followEnd();
      return;
    }
    if (pressed('m') && this.snapshot?.worker.canMessage) {
      this.composing = true;
      this.input.setValue('');
      this.status = '';
      this.mount();
      this.redraw();
      return;
    }
    if (pressed('x') && this.snapshot?.worker.canStop) {
      this.confirming = true;
      this.redraw();
      return;
    }
    // Enter does not dismiss a live transcript; Esc is the explicit exit.
    if (!this.bindings.matches(data, 'tui.select.confirm'))
      this.transcript.handleInput(data);
  }

  /** Input handlers cannot await; act() reports source failures itself, and anything
   * thrown while restoring the view lands in the status line rather than escaping. */
  private start(kind: 'message' | 'stop', text = ''): void {
    this.act(kind, text).catch((error: unknown) => {
      this.busy = false;
      if (!this.closed) this.status = labelText(String(error)).slice(0, 256);
    });
  }

  private async act(kind: 'message' | 'stop', text: string): Promise<void> {
    if (this.closed || this.busy || !this.selected) return;
    const id = this.selected;
    this.busy = true;
    this.confirming = false;
    this.status = '';
    this.redraw();
    try {
      const current = this.source.read(id)?.worker;
      if (!(kind === 'message' ? current?.canMessage : current?.canStop))
        throw new Error(
          'Worker is no longer controllable here; refresh its owner/state.',
        );
      if (kind === 'message') await this.source.message(id, text);
      else await this.source.stop(id);
      if (!this.closed) {
        this.composing = false;
        this.input.setValue('');
        this.status =
          kind === 'message' ? 'Message delivered' : 'Worker stopped';
      }
    } catch (error) {
      if (!this.closed) this.status = labelText(String(error)).slice(0, 256);
    } finally {
      this.busy = false;
      if (!this.closed) this.refresh();
    }
  }

  dispose(): void {
    this.closed = true;
    if (this.streamTimer) clearTimeout(this.streamTimer);
    this.streamTimer = undefined;
    this.stream = undefined;
    this.streamParts.clear();
    this.snapshot = undefined;
    this.clear();
  }
}
