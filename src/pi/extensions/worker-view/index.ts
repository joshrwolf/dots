import path from 'node:path';
import type {
  ExtensionAPI,
  ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {matchesKey, type Component} from '@earendil-works/pi-tui';
import type {RowModel} from '@joshrwolf/pi-kit/ui';
import {Inspector} from '../../src/ui/inspector.ts';
import {OutputView} from '../../src/ui/card.ts';
import {labelText} from '../../src/index.ts';
import {rexBinary} from '../rex/index.ts';
import {SessionFollower} from '../workers/follow.ts';
import {WorkerTranscript, transcriptOptions} from '../workers/transcript.ts';

/**
 * A read-only view of one worker's saved transcript, for a Pi started with
 * only this extension (see the Rex `workers_view` action). It reads the file
 * the worker's own Pi appends to and never submits a prompt: the view takes
 * the editor's place, and any input that still reaches Pi is handled here.
 * It shows saved records only; the response in flight appears once saved.
 */

/** Rows the view leaves to Pi's own chrome around the editor area. */
const RESERVED_ROWS = 3;

export default function register(pi: ExtensionAPI): void {
  pi.registerFlag('worker-session', {
    description: 'Session file of the worker transcript to show, read only',
    type: 'string',
  });
  pi.registerFlag('dismiss', {
    description: 'Rex action to run when the view closes',
    type: 'string',
  });

  let timer: ReturnType<typeof setInterval> | undefined;
  pi.on('input', () => ({action: 'handled'}));
  pi.on('session_shutdown', () => {
    if (timer) clearInterval(timer);
    timer = undefined;
  });
  pi.on('session_start', (_event, ctx) => {
    if (ctx.mode !== 'tui') return;
    const flag = pi.getFlag('worker-session');
    const file = typeof flag === 'string' && flag ? flag : undefined;
    const dismiss = pi.getFlag('dismiss');
    ctx.ui.setHeader(() => ({render: () => [], invalidate() {}}));
    ctx.ui.setFooter(() => ({render: () => [], invalidate() {}}));
    ctx.ui.setWorkingVisible(false);
    // Not awaited: session start completes while the view stays mounted.
    void ctx.ui
      .custom<void>((tui, theme, bindings, done) => {
        const transcript = file
          ? new WorkerTranscript(
              tui,
              new SessionFollower(file),
              transcriptOptions(pi.getSettings()),
            )
          : undefined;
        let closing = false;
        const close = () => {
          if (closing) return;
          closing = true;
          if (timer) clearInterval(timer);
          timer = undefined;
          transcript?.dispose();
          finish(
            ctx,
            typeof dismiss === 'string' ? dismiss : undefined,
            done,
          ).catch((error: unknown) => {
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
        const body: Component =
          transcript ??
          new OutputView('No worker session given (--worker-session FILE).');
        const footer = (): RowModel => ({
          layout: 'inline',
          groupSeparator: '  ',
          start: [
            {
              text: `${inspector.isFollowingEnd ? 'Following' : 'Paused'} · ↑/↓ scroll · F follow · ${bindings.getKeys('app.tools.expand').join('/')} expand · Esc/q close`,
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
            {
              title: `Worker · ${labelText(transcript?.name ?? path.basename(file ?? 'transcript'))}`,
              detail: 'read only',
              body,
              footer: footer(),
            },
            theme,
          );
        };
        update();
        if (transcript)
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
              transcript?.toggleExpanded();
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
        await pi.exec(
          rexBinary(process.env),
          ['--timeout', '30s', 'do', action, '--json', '--args', '{}'],
          {timeout: 35_000},
        );
      } catch {
        // Closing still ends the view; Rex then focuses a pane itself.
      }
    done();
    ctx.shutdown();
  }
}
