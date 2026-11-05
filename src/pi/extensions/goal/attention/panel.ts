import {
  getSelectListTheme,
  type KeybindingsManager,
  type Theme,
} from '@earendil-works/pi-coding-agent';
import {
  Input,
  MouseRegion,
  SelectList,
  Text,
  VStack,
  compositeTuiLine,
  matchesKey,
  truncateToWidth,
  type Component,
  type Focusable,
  type SelectItem,
  type SelectListTheme,
  type TuiMouseEvent,
} from '@earendil-works/pi-tui';
import {
  Panel,
  labelText,
  paintSurfaceLine,
  sourceText,
} from '@joshrwolf/pi-kit/ui';
import {Inspector} from '../../../src/ui/inspector.ts';
import {MarkdownView} from '../../../src/ui/markdown.ts';
import {PANEL_KEY, panelHeight, type GoalPanel} from '../panel.ts';
import type {AttentionItem, AttentionModel} from './model.ts';
import {answerText, coordinationNote, isOutstanding} from './model.ts';

/** What the person chose: a recorded choice, or their own words. */
export type Reply = {readonly choice: string} | {readonly text: string};

export interface AttentionActions {
  /** Persist the reply for an immutable request; the model reports the outcome. */
  readonly answer: (request: string, reply: Reply) => void;
  /** Forget a closed or replaced request and its draft. */
  readonly dismiss: (request: string) => void;
  /** In-memory drafts by request ID; they outlive this popup, not the runtime. */
  readonly drafts: Map<string, string>;
}

export type PopupView = 'attention' | 'work';
type Focus = 'requests' | 'choices' | 'text' | 'context';

/** Body rows the popup leaves for one view: Pi's overlay budget, less its tab and control rows. */
export const popupBodyHeight = (rows: number): number =>
  Math.max(1, panelHeight(rows) - 2);

/**
 * Side-by-side children that also route the mouse by column: HStack lays
 * out horizontally, but an overlay's mouse events reach it by rows only.
 * Fixed widths are cells; undefined shares the rest equally.
 */
class Columns implements Component {
  private readonly columns: readonly {
    component: Component;
    width?: number;
  }[];
  private readonly gap: number;
  constructor(
    columns: readonly {component: Component; width?: number}[],
    gap: number,
  ) {
    this.columns = columns;
    this.gap = gap;
  }
  private widths(width: number): number[] {
    const fixed = this.columns.reduce((sum, c) => sum + (c.width ?? 0), 0);
    const shared = this.columns.filter(c => c.width === undefined).length;
    const rest = Math.max(
      0,
      width - fixed - this.gap * Math.max(0, this.columns.length - 1),
    );
    return this.columns.map(c =>
      Math.max(0, c.width ?? Math.floor(rest / Math.max(1, shared))),
    );
  }
  render(width: number): string[] {
    const widths = this.widths(width);
    const rendered = this.columns.map((c, i) =>
      widths[i] ? c.component.render(widths[i]!) : [],
    );
    const height = Math.max(0, ...rendered.map(lines => lines.length));
    return Array.from({length: height}, (_, row) => {
      let line = '';
      let x = 0;
      rendered.forEach((lines, i) => {
        line = compositeTuiLine(line, lines[row] ?? '', x, widths[i]!, width);
        x += widths[i]! + this.gap;
      });
      return line;
    });
  }
  invalidate(): void {
    for (const c of this.columns) c.component.invalidate();
  }
  handleMouse(event: TuiMouseEvent) {
    const widths = this.widths(event.width);
    let x = 0;
    for (const [i, c] of this.columns.entries()) {
      const w = widths[i]!;
      if (event.x >= x && event.x < x + w)
        return c.component.handleMouse?.({...event, x: event.x - x, width: w});
      x += w + this.gap;
    }
    return undefined;
  }
}

const clickable = (component: Component, action: () => void): MouseRegion =>
  new MouseRegion(component, (event: TuiMouseEvent) => {
    if (event.button !== 'left') return undefined;
    if (event.type === 'press') return {handled: true, capture: true};
    if (event.type === 'click') {
      action();
      return {handled: true, focus: true, render: true};
    }
    return {handled: true};
  });

const glyph = (item: AttentionItem) =>
  item.state.kind === 'open'
    ? '?'
    : item.state.kind === 'saving'
      ? '…'
      : item.state.kind === 'answered'
        ? '✓'
        : '!';

/**
 * Requests, their question and options, and an answer field. The question
 * stays visible while typing; drafts survive selection changes, refreshes and
 * closing. Nothing here persists: the owner records replies and reports them.
 */
export class AttentionView implements Component, Focusable {
  private readonly bindings: Pick<KeybindingsManager, 'matches'>;
  private readonly actions: AttentionActions;
  private readonly rows: () => number;
  private readonly redraw: () => void;
  private readonly panel = new Panel();
  private readonly context: Inspector;
  private readonly inputs = new Map<string, Input>();
  private readonly choiceLists = new Map<
    string,
    {key: string; list: SelectList}
  >();
  private list: SelectList | undefined;
  private listKey = '';
  private model: AttentionModel = {items: [], unavailable: []};
  private theme: Theme | undefined;
  /**
   * The request the person is looking at, by ID. Selection never moves on
   * its own once made: saving, incorporation and refresh leave it on the same
   * request, and the next or replacement question needs a deliberate choice.
   */
  private selected: string | undefined;
  /** The selected request after it left the store's outstanding list. */
  private retired: AttentionItem | undefined;
  /** The person dismissed their selection; nothing is selected for them. */
  private cleared = false;
  private focus: Focus = 'requests';
  private isFocused = true;

  constructor(
    bindings: Pick<KeybindingsManager, 'matches'>,
    actions: AttentionActions,
    rows: () => number,
    redraw: () => void,
  ) {
    this.bindings = bindings;
    this.actions = actions;
    this.rows = rows;
    this.redraw = redraw;
    this.context = new Inspector(
      bindings,
      () => {
        this.focus = 'choices';
        this.redraw();
      },
      redraw,
      () => popupBodyHeight(this.rows()),
    );
  }

  get focused(): boolean {
    return this.isFocused;
  }
  set focused(value: boolean) {
    this.isFocused = value;
    this.syncFocus();
  }

  /** Text entry owns arrow keys and Escape. */
  get typing(): boolean {
    return this.focus === 'text';
  }
  /** At the request list, Escape closes rather than stepping back. */
  get atTop(): boolean {
    return this.focus === 'requests';
  }
  get outstanding(): number {
    return this.model.items.filter(item => isOutstanding(item)).length;
  }

  /** The selected request ID, for reopening on the same item. */
  get selection(): string | undefined {
    return this.current()?.request.id;
  }

  setModel(model: AttentionModel, theme: Theme, select?: string): void {
    const previous = this.current();
    this.model = model;
    this.theme = theme;
    if (select && model.items.some(item => item.request.id === select)) {
      this.selected = select;
      this.cleared = false;
    }
    if (
      previous &&
      previous.request.id === this.selected &&
      !model.items.some(item => item.request.id === this.selected)
    )
      this.retired =
        previous === this.retired
          ? previous
          : retire(previous, model.unavailable.length > 0);
    if (!this.current() && !this.cleared) {
      this.selected = model.items[0]?.request.id;
      this.focus = 'requests';
    }
    // A record that is no longer answerable leaves text entry.
    if (this.focus === 'text' && !this.answerable(this.current()))
      this.focus = 'choices';
    for (const id of this.inputs.keys())
      if (!this.entries().some(item => item.request.id === id))
        this.inputs.delete(id);
    this.invalidate();
  }

  /** Step back one level; false when already at the top. */
  back(): boolean {
    if (this.focus === 'text' || this.focus === 'context')
      this.focus = 'choices';
    else if (this.focus === 'choices') this.focus = 'requests';
    else return false;
    this.syncFocus();
    return true;
  }

  /** Restore list focus when the view is shown again. */
  reset(): void {
    if (this.focus !== 'text') this.focus = 'requests';
    this.syncFocus();
  }

  invalidate(): void {
    this.panel.invalidate();
    this.context.invalidate();
  }

  /** Listed requests, plus the retired selection while it stays selected. */
  private entries(): readonly AttentionItem[] {
    const retired = this.retired;
    return retired &&
      retired.request.id === this.selected &&
      !this.model.items.some(item => item.request.id === retired.request.id)
      ? [...this.model.items, retired]
      : this.model.items;
  }

  private current(): AttentionItem | undefined {
    return this.entries().find(item => item.request.id === this.selected);
  }

  private answerable(item: AttentionItem | undefined): boolean {
    return item?.state.kind === 'open' || item?.state.kind === 'error';
  }

  private syncFocus(): void {
    for (const [id, input] of this.inputs)
      input.focused =
        this.isFocused && this.focus === 'text' && id === this.selected;
  }

  private select(id: string): void {
    if (this.retired?.request.id !== id) this.retired = undefined;
    this.selected = id;
    this.cleared = false;
    this.focus = 'requests';
    this.syncFocus();
    this.redraw();
  }

  private listTheme(active: boolean): SelectListTheme {
    const native = getSelectListTheme();
    const theme = this.theme!;
    return {
      ...native,
      selectedPrefix: text =>
        active ? native.selectedPrefix(text) : ' '.repeat(text.length),
      selectedText: text =>
        active
          ? theme.bold(native.selectedText(text))
          : theme.fg('muted', text),
    };
  }

  private requestList(): SelectList {
    const key = JSON.stringify([
      this.focus === 'requests',
      this.entries().map(item => [item.request.id, glyph(item), item.goal]),
    ]);
    if (!this.list || key !== this.listKey) {
      this.listKey = key;
      const list = new SelectList(
        this.entries().map(item => ({
          value: item.request.id,
          label: `${glyph(item)} ${labelText(item.request.title)}`,
          description: labelText(item.goal),
        })),
        Math.max(1, Math.min(8, popupBodyHeight(this.rows()) - 1)),
        this.listTheme(this.focus === 'requests'),
      );
      list.setSelectedIndex(
        Math.max(
          0,
          this.entries().findIndex(item => item.request.id === this.selected),
        ),
      );
      list.onSelectionChange = item => this.select(item.value);
      // Enter on an unselected row selects it; only the shown request opens.
      list.onSelect = item => {
        if (item.value !== this.selected) return this.select(item.value);
        this.focus = 'choices';
        this.redraw();
      };
      this.list = list;
    }
    return this.list;
  }

  private input(item: AttentionItem): Input {
    const id = item.request.id;
    let input = this.inputs.get(id);
    if (!input) {
      input = new Input({prompt: 'Answer: ', placeholder: 'Your answer'});
      input.setValue(this.actions.drafts.get(id) ?? '');
      input.onSubmit = value => this.submit(item, value);
      input.onEscape = () => {
        this.focus = 'choices';
        this.syncFocus();
        this.redraw();
      };
      this.inputs.set(id, input);
    }
    input.focused = this.isFocused && this.focus === 'text';
    return input;
  }

  private submit(item: AttentionItem, value: string): void {
    const text = value.trim();
    if (!text || !this.answerable(item)) return;
    this.actions.drafts.set(item.request.id, value);
    this.actions.answer(item.request.id, {text});
    this.focus = 'choices';
    this.syncFocus();
    this.redraw();
  }

  private items(item: AttentionItem): SelectItem[] {
    const {request, state} = item;
    if (state.kind === 'stale')
      return [
        ...(state.replacement
          ? [{value: 'replacement', label: 'Answer the new question…'}]
          : []),
        {value: 'dismiss', label: 'Discard my draft'},
        {value: 'context', label: 'Read in full…'},
      ];
    if (state.kind === 'closed')
      return [
        {value: 'dismiss', label: 'Dismiss'},
        {value: 'context', label: 'Read in full…'},
      ];
    if (!this.answerable(item))
      return [{value: 'context', label: 'Read in full…'}];
    return [
      ...request.choices.map(choice => ({
        value: `choice:${choice.id}`,
        label: labelText(choice.label),
        ...(choice.detail ? {description: labelText(choice.detail)} : {}),
      })),
      {
        value: 'other',
        label: request.choices.length
          ? 'Type a different answer…'
          : 'Send an update…',
      },
      {value: 'context', label: 'Read in full…'},
    ];
  }

  private choices(item: AttentionItem): SelectList {
    const items = this.items(item);
    const key = JSON.stringify([
      item.state.kind,
      items.map(entry => entry.value),
    ]);
    const existing = this.choiceLists.get(item.request.id);
    if (existing?.key === key) return existing.list;
    // A changed record keeps the highlighted action when it remains; an
    // unanswerable or changed one rests on reading context, so a repeated
    // Enter never lands on an action the person did not see highlighted.
    const kept = existing?.list.getSelectedItem()?.value;
    const start = [kept, (existing || !this.answerable(item)) && 'context']
      .map(value => items.findIndex(entry => entry.value === value))
      .find(index => index >= 0);
    const native = getSelectListTheme();
    const theme = this.theme!;
    const list = new SelectList(items, 5, {
      ...native,
      selectedPrefix: text =>
        this.focus === 'requests'
          ? ' '.repeat(text.length)
          : native.selectedPrefix(text),
      selectedText: text =>
        this.focus === 'requests'
          ? theme.fg('text', text)
          : theme.bold(native.selectedText(text)),
    });
    if (start !== undefined) list.setSelectedIndex(start);
    list.onSelect = entry => this.choose(item.request.id, entry.value);
    this.choiceLists.set(item.request.id, {key, list});
    return list;
  }

  private choose(id: string, value: string): void {
    const item = this.entries().find(entry => entry.request.id === id);
    if (!item) return;
    if (value === 'context') {
      this.focus = 'context';
      this.context.setContent(
        {
          title: labelText(item.request.title),
          body: new MarkdownView(contextText(item), this.theme!),
          hint: '↑/↓ scroll · Enter/Esc back',
        },
        this.theme!,
        true,
      );
    } else if (value === 'other') {
      this.focus = 'text';
      this.input(item);
    } else if (value === 'dismiss') {
      this.selected = undefined;
      this.retired = undefined;
      this.cleared = true;
      this.focus = 'requests';
      this.actions.dismiss(id);
    } else if (value === 'replacement' && item.state.kind === 'stale') {
      const next = item.state.replacement?.id;
      if (next) this.select(next);
      return;
    } else if (value.startsWith('choice:') && this.answerable(item))
      this.actions.answer(id, {choice: value.slice('choice:'.length)});
    this.syncFocus();
    this.redraw();
  }

  /**
   * The selected request's question, state and actions, within `rows`. Prose
   * yields to the controls, which start at `control`.
   */
  private detail(
    item: AttentionItem,
    width: number,
    rows: number,
  ): {lines: string[]; control: number} {
    const theme = this.theme!;
    const {request, state} = item;
    const lines = (text: string, tone: Parameters<Theme['fg']>[0] = 'text') =>
      new Text(theme.fg(tone, text), 0, 0).render(width);
    const head = lines(labelText(item.goal), 'dim');
    const notes: string[] = [];
    if (state.kind === 'saving')
      notes.push(...lines('Saving your answer…', 'muted'));
    if (state.kind === 'answered')
      notes.push(
        ...lines(`Answer saved: ${answerText(request)}`, 'muted'),
        ...lines(coordinationNote(item), 'muted'),
      );
    if (state.kind === 'error')
      notes.push(...lines(`Not saved: ${state.message}`, 'warning'));
    if (state.kind === 'closed') notes.push(...lines(state.message, 'warning'));
    const draft = this.actions.drafts.get(request.id);
    if ((state.kind === 'stale' || state.kind === 'closed') && draft)
      notes.push(
        ...lines(`Your draft (not sent): ${labelText(draft)}`, 'muted'),
      );
    if (state.kind === 'stale')
      notes.push(
        ...lines(
          state.replacement
            ? `This question changed. New question: ${labelText(state.replacement.question)}`
            : 'This question was replaced.',
          'warning',
        ),
      );
    const typing = this.focus === 'text' && this.answerable(item);
    const tail = typing
      ? [
          ...this.input(item).render(width),
          ...new Text(theme.fg('accent', '[ Send answer ]'), 0, 0).render(
            width,
          ),
        ]
      : this.choices(item).render(width);
    const extra =
      !typing && this.rows() >= 28 && state.kind === 'open'
        ? lines(summaryText(request), 'text')
        : [];
    const question = new Text(
      theme.bold(sourceText(request.question)),
      0,
      0,
    ).render(width);
    const fit = (text: readonly string[], room: number) =>
      text.length <= room
        ? text
        : [
            ...text.slice(0, Math.max(0, room - 1)),
            truncateToWidth(
              theme.fg('dim', '… Read in full for the rest'),
              width,
            ),
          ];
    const room = Math.max(0, rows - tail.length);
    const shown = fit(question, Math.max(1, room - head.length - notes.length));
    const summary = fit(
      extra,
      Math.max(0, room - head.length - notes.length - shown.length),
    );
    const prose = [...head, ...shown, ...summary, ...notes].slice(0, room);
    return {
      lines: [...prose, ...tail].slice(0, rows),
      control: prose.length,
    };
  }

  /** Mouse targets mirror the rendered layout. */
  private body(width: number, rows: number): Component {
    const theme = this.theme!;
    const item = this.current();
    if (!item)
      return new Text(
        theme.fg(
          'muted',
          this.model.unavailable.length
            ? `Attention unavailable: ${this.model.unavailable.map(labelText).join('; ')}`
            : this.cleared && this.model.items.length
              ? 'Choose a request: ↑/↓ or Enter.'
              : 'Nothing needs your answer.',
        ),
        0,
        0,
      );
    const split = width >= 100;
    const detailWidth = split ? width - 33 : width;
    const position = split
      ? []
      : this.focus === 'requests'
        ? [
            truncateToWidth(
              theme.fg(
                'muted',
                `${this.entries().indexOf(item) + 1}/${this.entries().length} · `,
              ) + theme.fg('accent', theme.bold(labelText(item.request.title))),
              width,
            ),
          ]
        : [];
    const {lines, control} = this.detail(
      item,
      detailWidth,
      rows - position.length,
    );
    const tail = this.focus === 'text' && this.answerable(item);
    const detail = this.mouseDetail(item, lines, control, tail);
    if (!split)
      return new VStack([
        ...position.map(line => new Text(line, 0, 0)),
        detail,
      ]);
    const left = new VStack([
      new Text(
        theme.fg(this.focus === 'requests' ? 'accent' : 'muted', 'Requests'),
        0,
        0,
      ),
      this.requestList(),
    ]);
    return new Columns([{component: left, width: 30}, {component: detail}], 3);
  }

  /**
   * Detail lines, with the native choice list or input taking mouse events
   * on exactly the rows it rendered, from `control`; prose takes none.
   */
  private mouseDetail(
    item: AttentionItem,
    lines: readonly string[],
    control: number,
    typing: boolean,
  ): Component {
    return {
      render: () => [...lines],
      invalidate() {},
      handleMouse: (event: TuiMouseEvent) => {
        if (event.y < control || event.y >= lines.length) return undefined;
        const y = event.y - control;
        if (typing) {
          if (y === 1) {
            if (event.button === 'left' && event.type === 'press')
              return {handled: true, capture: true};
            if (event.button === 'left' && event.type === 'click') {
              this.submit(item, this.input(item).getValue());
              return {handled: true, render: true};
            }
            return {handled: true};
          }
          const result = this.input(item).handleMouse({...event, y});
          if (result?.handled) {
            this.focus = 'text';
            this.syncFocus();
          }
          return result;
        }
        const list = this.choices(item);
        const result = list.handleMouse({...event, y});
        // A click selects the option natively through onSelect.
        if (result?.handled && this.focus === 'requests')
          this.focus = 'choices';
        return result ? {...result, render: true} : result;
      },
    };
  }

  render(width: number): string[] {
    const theme = this.theme;
    if (!theme || width <= 0) return [];
    const rows = popupBodyHeight(this.rows());
    if (this.focus === 'context') return this.context.render(width);
    const inner = width < 12 ? width : width - 4;
    this.panel.update(
      {
        heading: {
          start: [
            {
              text: 'Needs your attention',
              tone: 'accent',
              bold: true,
              priority: 100,
              shrink: 'clip',
            },
          ],
          end: [
            {
              text: `${this.outstanding} outstanding`,
              tone: 'dim',
              priority: 1,
              shrink: 'omit',
            },
          ],
        },
        body: new BoundedBody(this.body(inner, rows), rows),
        footer: {
          start: [
            {
              text:
                this.focus === 'text'
                  ? 'Enter send · Esc back · ←/→ edit answer'
                  : this.focus === 'requests'
                    ? '← Attention · → Work · ↑/↓ request · Enter answer · Esc close'
                    : '← Attention · → Work · ↑/↓ choice · Enter select · Esc back',
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
    return this.panel.render(width);
  }

  handleMouse(event: TuiMouseEvent) {
    return (this.focus === 'context' ? this.context : this.panel).handleMouse?.(
      event,
    );
  }

  handleInput(data: string): void {
    const item = this.current();
    if (this.focus === 'text' && item) {
      const input = this.input(item);
      input.handleInput(data);
      if (this.focus === 'text')
        this.actions.drafts.set(item.request.id, input.getValue());
    } else if (this.focus === 'context') this.context.handleInput(data);
    else if (this.focus === 'requests') {
      if (this.bindings.matches(data, 'tui.select.confirm') && item)
        this.focus = 'choices';
      else this.requestList().handleInput(data);
    } else if (item) this.choices(item).handleInput(data);
    this.redraw();
  }
}

/** Never more than the view's row budget, so short screens keep the frame and options. */
class BoundedBody implements Component {
  private readonly inner: Component;
  private readonly rows: number;
  constructor(inner: Component, rows: number) {
    this.inner = inner;
    this.rows = rows;
  }
  render(width: number): string[] {
    return this.inner.render(width).slice(0, this.rows);
  }
  invalidate(): void {
    this.inner.invalidate();
  }
  handleMouse(event: TuiMouseEvent) {
    return this.inner.handleMouse?.(event);
  }
}

/**
 * A selected request that left the outstanding list stays, unanswerable.
 * While part of the scope is unreadable, its absence proves nothing.
 */
function retire(item: AttentionItem, unread: boolean): AttentionItem {
  if (item.state.kind === 'stale' || item.state.kind === 'closed') return item;
  const saved = item.request.answer
    ? `Answer saved: ${answerText(item.request)}. `
    : '';
  const status = unread
    ? 'Could not be checked just now; your draft is kept.'
    : 'No longer outstanding.';
  return {...item, state: {kind: 'closed', message: `${saved}${status}`}};
}

function summaryText(request: AttentionItem['request']): string {
  return [
    request.context && sourceText(request.context),
    request.recommendation &&
      `Recommend: ${sourceText(request.recommendation)}`,
    request.tasks.length &&
      `Waits: ${request.tasks.map(task => labelText(task.title)).join(', ')}`,
  ]
    .filter(Boolean)
    .join('\n\n');
}

function contextText(item: AttentionItem): string {
  const {request} = item;
  return [
    request.question,
    request.context,
    request.recommendation && `**Recommend:** ${request.recommendation}`,
    // Every choice's full label and detail; the list may clip them.
    request.choices.length &&
      `Choices:\n${request.choices
        .map(
          choice =>
            `- **${choice.label}**${choice.detail ? `: ${choice.detail}` : ''}`,
        )
        .join('\n')}`,
    request.tasks.length &&
      `Waiting work:\n${request.tasks.map(task => `- ${task.title} (${task.status})`).join('\n')}`,
    `Goal: ${item.goal} · request ${request.id}`,
  ]
    .filter(Boolean)
    .join('\n\n');
}

export interface PopupParts {
  readonly attention: AttentionView;
  /** Its close action must call the popup's back(). */
  readonly work: GoalPanel;
  readonly close: () => void;
  /** Reports the shown view so the next ordinary reopen restores it. */
  readonly viewed: (view: PopupView) => void;
}

/**
 * One popup with Attention and Work views. Left/Right switch views except
 * while typing, where they move the cursor; there is no separate focus layer.
 * Escape steps back, then closes; closing never answers or withdraws.
 */
export class GoalPopup implements Component, Focusable {
  private readonly parts: PopupParts;
  private view: PopupView;
  private isFocused = true;
  private readonly root = new VStack();

  constructor(parts: PopupParts, view: PopupView) {
    this.parts = parts;
    this.view = view;
  }

  get current(): PopupView {
    return this.view;
  }
  get focused(): boolean {
    return this.isFocused;
  }
  set focused(value: boolean) {
    this.isFocused = value;
    this.parts.attention.focused = value && this.view === 'attention';
  }

  show(view: PopupView): void {
    this.view = view;
    this.parts.viewed(view);
    this.parts.attention.focused = this.isFocused && view === 'attention';
    if (view === 'attention') this.parts.attention.reset();
  }

  private close(): void {
    this.parts.viewed(this.view);
    this.parts.close();
  }

  /** Escape: one level back, Work to Attention while requests remain, then close. */
  back(): void {
    if (this.view === 'attention' && this.parts.attention.back()) return;
    if (this.view === 'work' && this.parts.attention.outstanding) {
      this.show('attention');
      return;
    }
    this.close();
  }

  invalidate(): void {
    this.parts.attention.invalidate();
    this.parts.work.invalidate();
  }

  render(width: number): string[] {
    const theme = this.theme();
    this.root.clear();
    if (!theme) return [];
    const button = (label: string, action: () => void, selected = false) =>
      clickable(
        new Text(
          theme.fg(
            selected ? 'accent' : 'muted',
            `${selected ? '› ' : ''}[ ${label} ]`,
          ),
          0,
          0,
        ),
        () => {
          action();
        },
      );
    this.root.addChild(
      new Columns(
        [
          button(
            `← Attention (${this.parts.attention.outstanding})`,
            () => this.show('attention'),
            this.view === 'attention',
          ),
          button('Work →', () => this.show('work'), this.view === 'work'),
        ].map(component => ({component})),
        2,
      ),
    );
    this.root.addChild(
      this.view === 'attention' ? this.parts.attention : this.parts.work,
    );
    const top = this.view === 'attention' && this.parts.attention.atTop;
    this.root.addChild(
      new Columns(
        [
          ...(top ? [] : [button('Back', () => this.back())]),
          button('Close', () => this.close()),
        ].map(component => ({component})),
        2,
      ),
    );
    const colors = {
      fg: theme.getFgAnsi('text'),
      bg: theme.getBgAnsi('customMessageBg'),
    };
    // The tab and control rows share the popup's surface.
    const lines = this.root.render(width);
    const last = lines.length - 1;
    return lines.map((line, row) =>
      row === 0 || row === last ? paintSurfaceLine(line, width, colors) : line,
    );
  }

  private theme(): Theme | undefined {
    return this.themeSource?.();
  }
  /** Set by the owner: the active theme at render time. */
  themeSource: (() => Theme) | undefined;

  handleMouse(event: TuiMouseEvent) {
    return this.root.handleMouse(event);
  }

  handleInput(data: string): void {
    const attention = this.parts.attention;
    if (matchesKey(data, PANEL_KEY)) {
      this.close();
      return;
    }
    if (this.view === 'attention' && attention.typing) {
      attention.handleInput(data);
      return;
    }
    // The work view steps back through its own inspections, then calls back().
    if (matchesKey(data, 'escape') && this.view === 'attention') {
      this.back();
      return;
    }
    if (matchesKey(data, 'left') || matchesKey(data, 'right')) {
      this.show(matchesKey(data, 'left') ? 'attention' : 'work');
      return;
    }
    if (this.view === 'work') this.parts.work.handleInput(data);
    else attention.handleInput(data);
  }
}
