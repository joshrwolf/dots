import {
  keyText,
  type ExtensionAPI,
  type ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {matchesKey, type Component} from '@earendil-works/pi-tui';
import {labelText, type RowModel} from '@joshrwolf/pi-kit/ui';
import {Inspector} from '../../src/ui/inspector.ts';
import {rexDo} from '../rex/index.ts';
import {SessionFollower} from '../workers/follow.ts';
import {WorkerTranscript, transcriptOptions} from '../workers/transcript.ts';

/**
 * A read-only view of one worker's saved transcript, for a Pi started with
 * this extension and `--worker-session` (see the Rex `workers_view` action).
 * It reads the file the worker's own Pi appends to and never submits a
 * prompt: the view takes the editor's place, and any input that still
 * reaches Pi is handled here. It shows saved records only; the response in
 * flight appears once saved. Without `--worker-session` it does nothing.
 */

/** Rows the view leaves to Pi's own chrome around the editor area. */
const RESERVED_ROWS = 3;

export default function register(pi: ExtensionAPI): void {
  pi.registerFlag('worker-session', {
    description: 'Session file of the worker transcript to show, read only',
    type: 'string',
  });
  pi.registerFlag('worker-label', {
    description: 'Worker label the transcript view shows as its title',
    type: 'string',
  });
  pi.registerFlag('dismiss', {
    description: 'Rex action to run when the view closes',
    type: 'string',
  });

  const text = (name: string) => {
    const value = pi.getFlag(name);
    return typeof value === 'string' && value ? value : undefined;
  };
  let viewing = false;
  let timer: ReturnType<typeof setInterval> | undefined;
  pi.on('input', () => (viewing ? {action: 'handled'} : undefined));
  pi.on('session_shutdown', () => {
    if (timer) clearInterval(timer);
    timer = undefined;
  });
  pi.on('session_start', (_event, ctx) => {
    const file = text('worker-session');
    viewing = file !== undefined;
    if (!file || ctx.mode !== 'tui') return;
    const title = `Worker · ${labelText(text('worker-label') ?? 'worker')}`;
    const dismiss = text('dismiss');
    ctx.ui.setHeader(() => ({render: () => [], invalidate() {}}));
    ctx.ui.setFooter(() => ({render: () => [], invalidate() {}}));
    ctx.ui.setWorkingVisible(false);
    // Not awaited: session start completes while the view stays mounted.
    void ctx.ui
      .custom<void>((tui, theme, bindings, done) => {
        const transcript = new WorkerTranscript(
          tui,
          new SessionFollower(file),
          transcriptOptions(pi.getSettings()),
        );
        let closing = false;
        const close = () => {
          if (closing) return;
          closing = true;
          if (timer) clearInterval(timer);
          timer = undefined;
          transcript.dispose();
          finish(ctx, dismiss, done).catch((error: unknown) => {
            ctx.ui.notify(
              `Worker view failed to close: ${String(error)}`,
              'error',
            );
          });
        };
        const height = () => Math.max(1, tui.terminal.rows - RESERVED_ROWS - 2);
        const inspector = new Inspector(
          bindings,
          close,
          () => tui.requestRender(),
          height,
          'end',
        );
        const footer = (): RowModel => ({
          layout: 'inline',
          groupSeparator: '  ',
          start: [
            {
              text: `${inspector.isFollowingEnd ? 'Following' : 'Paused'} · ↑/↓ scroll · F follow · ${keyText('app.tools.expand')} expand · Esc/q close`,
              tone: 'dim',
              priority: 100,
              shrink: 'clip',
            },
          ],
          end: [
            {
              text: 'saved records only; streaming not shown',
              tone: 'dim',
              priority: 10,
              shrink: 'omit',
            },
          ],
        });
        let shownFollowing: boolean | undefined;
        const update = () => {
          if (shownFollowing === inspector.isFollowingEnd) return;
          shownFollowing = inspector.isFollowingEnd;
          inspector.setContent(
            {title, detail: 'read only', body: transcript, footer: footer()},
            theme,
          );
        };
        update();
        timer = setInterval(() => {
          if (transcript.refresh()) {
            shownFollowing = undefined;
            update();
            tui.requestRender();
          }
        }, 1000);
        return {
          render(width: number): string[] {
            update();
            return inspector.render(width);
          },
          invalidate() {
            inspector.invalidate();
          },
          handleInput(data: string) {
            if (
              bindings.matches(data, 'tui.select.cancel') ||
              matchesKey(data, 'q') ||
              matchesKey(data, 'ctrl+c')
            ) {
              close();
              return;
            }
            if (matchesKey(data, 'f') || matchesKey(data, 'shift+f')) {
              inspector.followEnd();
              return;
            }
            if (bindings.matches(data, 'app.tools.expand')) {
              transcript.toggleExpanded();
              return;
            }
            // Enter does not close a live transcript; Esc and q do.
            if (!bindings.matches(data, 'tui.select.confirm'))
              inspector.handleInput(data);
          },
        } satisfies Component & {handleInput(data: string): void};
      })
      .catch((error: unknown) => {
        ctx.ui.notify(`Worker view failed: ${String(error)}`, 'error');
      });
  });

  /** Runs the caller's dismiss action, then leaves Pi. */
  async function finish(
    ctx: ExtensionContext,
    action: string | undefined,
    done: (result: void) => void,
  ): Promise<void> {
    if (action)
      try {
        await rexDo(pi, action);
      } catch {
        // Closing still ends the view; Rex then focuses a pane itself.
      }
    done();
    ctx.shutdown();
  }
}
