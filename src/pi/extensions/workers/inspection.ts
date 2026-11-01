import type {ExtensionContext} from '@earendil-works/pi-coding-agent';
import {inspectorHeight, inspectorOverlay} from '../../src/ui/inspector.ts';
import {WorkerInspector, type WorkerInspectionSource} from './inspector.ts';

/** One session-bound inspection overlay. File reads and actions belong to the source. */
export class WorkerInspection {
  private closed = false;
  private opening = false;
  private close: (() => void) | undefined;
  private view: WorkerInspector | undefined;
  private refreshTimer: ReturnType<typeof setInterval> | undefined;

  async open(
    ctx: ExtensionContext,
    source: WorkerInspectionSource,
    id?: string,
  ): Promise<void> {
    if (this.closed || this.opening) return;
    if (ctx.mode !== 'tui') {
      ctx.ui.notify('Workers require interactive mode', 'info');
      return;
    }
    this.opening = true;
    try {
      await ctx.ui.custom<void>((tui, theme, bindings, done) => {
        const finish = () => done();
        if (this.closed) {
          done();
          return {render: () => [], invalidate() {}};
        }
        this.close = finish;
        const view = new WorkerInspector(
          source,
          theme,
          bindings,
          finish,
          () => tui.requestRender(),
          () => inspectorHeight(tui.terminal.rows),
          id,
        );
        this.view = view;
        // Descendant registry entries and saved transcripts change in other processes.
        // Only the open overlay polls, never a row or a renderer.
        this.refreshTimer = setInterval(() => view.refresh(), 1000);
        return view;
      }, inspectorOverlay);
    } finally {
      if (this.refreshTimer) clearInterval(this.refreshTimer);
      this.refreshTimer = undefined;
      this.view?.dispose();
      this.view = undefined;
      this.opening = false;
      this.close = undefined;
    }
  }

  refresh(): void {
    this.view?.refresh();
  }
  record(
    id: string,
    record: {type: string; message?: unknown; assistantMessageEvent?: unknown},
  ): void {
    this.view?.record(id, record);
  }

  dispose(): void {
    this.closed = true;
    if (this.refreshTimer) clearInterval(this.refreshTimer);
    this.refreshTimer = undefined;
    this.view?.dispose();
    this.close?.();
    this.close = undefined;
  }
}
