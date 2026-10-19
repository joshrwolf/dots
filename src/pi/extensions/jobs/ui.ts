import type { ExtensionContext, KeybindingsManager, Theme } from "@earendil-works/pi-coding-agent";
import { ScrollView, Text, VStack, matchesKey } from "@earendil-works/pi-tui";
import { OutputView, labelText } from "../../src/index.ts";
import { MAX_RETAINED, type Job } from "./protocol.ts";
import { JobsWidget, canStop, inspectCard, projectJob, type JobView } from "./present.ts";

export const JOBS_WIDGET = "dots.jobs";
export interface JobSource {
  list(): Job[];
  get(id: string): Job;
  hasLive(): boolean;
  stop(id: string): Job;
}
type UIContext = Pick<ExtensionContext, "mode" | "hasUI"> & {
  ui: Pick<ExtensionContext["ui"], "setWidget" | "select" | "confirm" | "notify" | "custom">;
};

/** Native layout/scrolling; no reads, clocks, or process/session access in render. */
export class JobInspector extends VStack {
  private readonly body = new VStack();
  private readonly scroll = new ScrollView(this.body, { scrollbar: "always", overscroll: "contain" });
  private readonly bindings: Pick<KeybindingsManager, "matches">;
  private readonly close: () => void;
  private readonly redraw: () => void;
  constructor(model: JobView, theme: Theme, bindings: Pick<KeybindingsManager, "matches">, close: () => void, redraw: () => void) {
    super();
    this.bindings = bindings; this.close = close; this.redraw = redraw;
    this.addChild(this.scroll, { basis: 20, grow: 1, minSize: 1, maxSize: 24 });
    this.addChild(new Text("↑/↓ PgUp/PgDn scroll · Enter/Esc close", 0, 0));
    this.update(model, theme);
  }
  update(model: JobView | undefined, theme: Theme): void {
    this.body.clear();
    this.body.addChild(model ? inspectCard(model, theme) : new OutputView("Job is no longer retained in this history."));
    this.invalidate();
  }
  handleInput(data: string): void {
    if (this.bindings.matches(data, "tui.select.cancel") || this.bindings.matches(data, "tui.select.confirm")) { this.close(); return; }
    if (matchesKey(data, "up")) this.scroll.scrollBy(-1);
    else if (matchesKey(data, "down")) this.scroll.scrollBy(1);
    else if (matchesKey(data, "pageUp")) this.scroll.scrollBy(-Math.max(1, this.scroll.viewportHeight));
    else if (matchesKey(data, "pageDown")) this.scroll.scrollBy(Math.max(1, this.scroll.viewportHeight));
    else if (matchesKey(data, "home")) this.scroll.scrollToStart();
    else if (matchesKey(data, "end")) this.scroll.scrollToEnd();
    this.redraw();
  }
}

/** One session/branch UI owner. A single 1s refresh exists only for live TUI jobs. */
export class JobsPresentation {
  private readonly ctx: UIContext;
  private readonly source: JobSource;
  private closed = false;
  private timer: ReturnType<typeof setInterval> | undefined;
  private widget: { view: JobsWidget; theme: Theme; redraw: () => void } | undefined;
  private mounted = false;
  private models: readonly JobView[] = [];
  private dialog: AbortController | undefined;
  private inspector: { id: string; view: JobInspector; theme: Theme; redraw: () => void; close: () => void } | undefined;

  constructor(ctx: UIContext, source: JobSource) { this.ctx = ctx; this.source = source; }
  private enabled(): boolean { return this.ctx.mode === "tui" && this.ctx.hasUI; }
  private stopRefresh(): void { clearInterval(this.timer); this.timer = undefined; }

  refresh(): void {
    if (this.closed || !this.enabled()) return;
    try { this.refreshSnapshot(); }
    catch (error) {
      // Presentation failure must not turn successful job admission into a failed
      // tool result, or escape a timer callback and jeopardize owned processes.
      this.dispose(false);
      try { this.ctx.ui.notify(`Jobs display unavailable: ${labelText(String(error)).slice(0, 256)}`, "warning"); }
      catch { /* A stale/failed UI cannot be used to report its own failure. */ }
    }
  }
  private refreshSnapshot(): void {
    this.models = Object.freeze(this.source.list().slice(0, MAX_RETAINED * 2).map(projectJob));
    const visible = this.models.some((m) => m.live || m.state === "unknown");
    if (visible && !this.mounted) {
      this.mounted = true;
      this.ctx.ui.setWidget(JOBS_WIDGET, (tui, theme) => {
        if (this.closed) return { render: () => [], invalidate() {} };
        const owned = { view: new JobsWidget(), theme, redraw: () => tui.requestRender() };
        owned.view.update(this.models, theme);
        this.widget = owned;
        return {
          render: (width) => this.closed ? [] : owned.view.render(width),
          invalidate: () => owned.view.invalidate(),
          dispose: () => {
            if (this.widget !== owned) return;
            this.widget = undefined;
            this.mounted = false;
            this.stopRefresh();
          },
        };
      });
    } else if (!visible && this.mounted) {
      this.ctx.ui.setWidget(JOBS_WIDGET, undefined);
      this.widget = undefined;
      this.mounted = false;
    }
    this.widget?.view.update(this.models, this.widget.theme);
    this.widget?.redraw();
    if (this.inspector) {
      this.inspector.view.update(this.models.find((m) => m.id === this.inspector?.id), this.inspector.theme);
      this.inspector.redraw();
    }
    if (this.source.hasLive()) {
      if (!this.timer) { this.timer = setInterval(() => this.refresh(), 1000); this.timer.unref(); }
    } else this.stopRefresh();
  }

  async open(id?: string): Promise<void> {
    if (this.closed || !this.enabled()) return;
    if (this.dialog) { this.ctx.ui.notify("A jobs picker is already open.", "info"); return; }
    const dialog = new AbortController();
    this.dialog = dialog;
    const options = { signal: dialog.signal };
    const active = () => !this.closed && !dialog.signal.aborted;
    try {
      if (!id) {
        const models = this.source.list().slice(0, MAX_RETAINED * 2).map(projectJob);
        if (!models.length) { this.ctx.ui.notify("No retained jobs.", "info"); return; }
        const choices = models.map((m) => `inspect ${m.subject} · ${m.state} · ${m.id}`);
        const selected = await this.ctx.ui.select("Jobs", choices, options);
        if (!active() || selected === undefined) return;
        id = models[choices.indexOf(selected)]?.id;
        if (!id) return;
      }
      const initial = projectJob(this.source.get(id));
      const action = await this.ctx.ui.select(`Job ${initial.id}`, initial.stoppable ? ["Inspect", "Stop job"] : ["Inspect"], options);
      if (!active() || action === undefined) return;
      if (action === "Stop job") {
        // Re-read after every awaited interaction. Displayed history never grants control.
        if (!canStop(this.source.get(id))) { this.ctx.ui.notify("Job is no longer stoppable; inspect its latest state.", "info"); return; }
        const approved = await this.ctx.ui.confirm("Stop job?", `${initial.id}\n${initial.subject}\nTERM then fixed 3-second KILL grace.`, options);
        if (!active() || !approved) return;
        if (!canStop(this.source.get(id))) { this.ctx.ui.notify("Job is no longer stoppable; inspect its latest state.", "info"); return; }
        this.source.stop(id);
        this.refresh();
        this.ctx.ui.notify(`Stop requested for ${initial.id}; cleanup is not yet confirmed.`, "info");
      } else if (action === "Inspect") {
        const selectedId = id;
        await this.ctx.ui.custom<void>((tui, theme, bindings, done) => {
          if (!active()) { done(); return { render: () => [], invalidate() {} }; }
          const view = new JobInspector(projectJob(this.source.get(selectedId)), theme, bindings, () => done(), () => tui.requestRender());
          const owned = { id: selectedId, view, theme, redraw: () => tui.requestRender(), close: () => done() };
          this.inspector = owned;
          // Return the native stack itself so its layout/scroll nodes remain visible.
          return Object.assign(view, { dispose: () => { if (this.inspector === owned) this.inspector = undefined; } });
        });
      }
    } catch (error) {
      if (active()) this.ctx.ui.notify(labelText(String(error)).slice(0, 512), "error");
    } finally {
      if (this.dialog === dialog) this.dialog = undefined;
    }
  }

  /** On shutdown Pi owns surface reset; tree navigation removes only our keyed widget. */
  dispose(removeWidget = true): void {
    if (this.closed) return;
    this.closed = true;
    this.stopRefresh();
    this.dialog?.abort();
    this.dialog = undefined;
    this.inspector?.close();
    this.inspector = undefined;
    if (removeWidget && this.mounted && this.enabled()) this.ctx.ui.setWidget(JOBS_WIDGET, undefined);
    this.widget = undefined;
    this.mounted = false;
    this.models = [];
  }
}
