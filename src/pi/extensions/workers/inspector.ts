import {
  getSelectListTheme,
  keyText,
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
  type TUI,
} from '@earendil-works/pi-tui';
import {Panel, type RowModel} from '@joshrwolf/pi-kit/ui';
import {Inspector} from '../../src/ui/inspector.ts';
import {OutputView} from '../../src/ui/card.ts';
import {labelText} from '../../src/index.ts';
import {SessionFollower} from './follow.ts';
import type {WorkerEvent} from './protocol.ts';
import {WorkerTranscript, type TranscriptOptions} from './transcript.ts';

export interface WorkerListItem {
  id: string;
  label: string;
  state: string;
  depth: number;
  owner?: string;
  canMessage: boolean;
  canStop: boolean;
}
export interface WorkerSnapshot {
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
  /** Opens the worker's saved transcript in a Rex view; absent outside Rex. */
  openInRex?(id: string): Promise<void>;
}

/** What the inspector borrows from the overlay that hosts it. */
export interface WorkerInspectorHost {
  readonly tui: TUI;
  readonly theme: Theme;
  readonly bindings: Pick<KeybindingsManager, 'matches'>;
  readonly close: () => void;
  /** Transcript body rows. */
  readonly height: () => number;
  readonly transcript: TranscriptOptions;
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
  private readonly host: WorkerInspectorHost;
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
  private view: WorkerTranscript | undefined;
  private bodyKey = '';
  private composing = false;
  private confirming = false;
  private busy = false;
  private closed = false;
  /** The last action's outcome or failure. */
  private status = '';
  /** Why the list may be incomplete. */
  private listNotice = '';
  private hasFocus = false;

  constructor(
    source: WorkerInspectionSource,
    host: WorkerInspectorHost,
    id?: string,
  ) {
    super();
    this.source = source;
    this.host = host;
    this.theme = host.theme;
    this.bindings = host.bindings;
    this.close = host.close;
    this.redraw = () => host.tui.requestRender();
    this.height = host.height;
    this.transcript = new Inspector(
      host.bindings,
      host.close,
      this.redraw,
      host.height,
      'end',
    );
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
        this.follow(this.snapshot?.sessionFile);
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
        this.listNotice = [
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
      const notice = `Inspection unavailable: ${labelText(String(error)).slice(0, 256)}`;
      if (this.selected) {
        this.status = notice;
        this.updateTranscript();
      } else this.listNotice = notice;
    }
    this.mount();
    this.redraw();
  }

  /** One follower per selected session file; a new file starts a new transcript. */
  private follow(file: string | undefined): void {
    if (this.view && this.view.file === file) {
      this.view.refresh();
      return;
    }
    this.view?.dispose();
    this.view = new WorkerTranscript(
      this.host.tui,
      new SessionFollower(file),
      this.host.transcript,
    );
    this.bodyKey = '';
  }

  private unfollow(): void {
    this.view?.dispose();
    this.view = undefined;
    this.bodyKey = '';
  }

  private select(id: string): void {
    this.selected = id;
    this.snapshot = undefined;
    this.unfollow();
    this.status = '';
    this.confirming = false;
    this.transcript.followEnd();
    this.refresh();
  }

  /** The selected worker's live RPC events; saved records arrive through refresh. */
  record(id: string, record: WorkerEvent): void {
    if (this.closed || id !== this.selected) return;
    this.view?.record(record);
    if (record.type === 'message_end' || record.type === 'agent_settled')
      this.refresh();
  }

  private updateTranscript(): void {
    const snapshot = this.snapshot;
    const key = JSON.stringify([
      snapshot?.detail,
      snapshot?.worker,
      snapshot?.sessionFile,
    ]);
    if (key === this.bodyKey) return;
    this.bodyKey = key;
    const body = new VStack();
    if (!snapshot || !this.view)
      body.addChild(
        new OutputView('Worker unavailable. Return to the list to refresh.'),
      );
    else {
      body.addChild(
        new OutputView(
          `${snapshot.detail}\n${readOnly(snapshot.worker)}Saved transcript: ${snapshot.sessionFile ?? 'not saved yet'}`,
        ),
      );
      body.addChild(this.view);
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
    const notice = labelText(
      this.status || (this.selected ? '' : this.listNotice),
    );
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
            ? `${this.isFollowingEnd ? 'Following' : 'Paused'} · ↑/↓ scroll · F follow · ${keyText('app.tools.expand')} expand${worker?.canMessage ? ' · M message' : ''}${worker?.canStop ? ' · X stop' : ''}${this.rex ? ' · R open in Rex' : ''} · L list · Esc close`
            : `↑/↓ select · Enter inspect${this.rex ? ' · R open in Rex' : ''} · Esc close`;
    const key = JSON.stringify([
      this.selected,
      this.bodyKey,
      this.rosterKey,
      this.pickerRows,
      this.composing,
      hint,
      this.status,
      this.listNotice,
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
                text: `${this.items.length} listed`,
                tone: 'muted',
                priority: 10,
                shrink: 'omit',
              },
            ],
          },
          body:
            this.items.length && this.picker
              ? this.picker
              : new Text('No workers listed.', 0, 0),
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
    const pressed = (key: 'f' | 'l' | 'm' | 'r' | 'x') =>
      matchesKey(data, key) || matchesKey(data, `shift+${key}`);
    if (!this.selected) {
      const item = this.picker?.getSelectedItem();
      if (pressed('r') && this.rex && item) {
        this.start('rex', '', item.value);
        return;
      }
      this.picker?.handleInput(data);
      this.redraw();
      return;
    }
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
      this.unfollow();
      this.status = '';
      this.refresh();
      return;
    }
    if (pressed('r') && this.rex) {
      this.start('rex', '', this.selected);
      return;
    }
    if (this.bindings.matches(data, 'app.tools.expand')) {
      this.view?.toggleExpanded();
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
  private start(
    kind: 'message' | 'stop' | 'rex',
    text = '',
    id = this.selected,
  ): void {
    this.act(kind, text, id).catch((error: unknown) => {
      this.busy = false;
      if (!this.closed) this.status = labelText(String(error)).slice(0, 256);
    });
  }

  private get rex(): boolean {
    return this.source.openInRex !== undefined;
  }

  private async act(
    kind: 'message' | 'stop' | 'rex',
    text: string,
    id: string | undefined,
  ): Promise<void> {
    if (this.closed || this.busy || !id) return;
    this.busy = true;
    this.confirming = false;
    this.status = '';
    this.redraw();
    try {
      if (kind === 'rex') {
        await this.source.openInRex?.(id);
        if (!this.closed) this.status = 'Opened in Rex';
        return;
      }
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
    this.unfollow();
    this.snapshot = undefined;
    this.clear();
  }
}
