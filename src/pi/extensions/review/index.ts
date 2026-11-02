import {watch} from 'node:fs';
import path from 'node:path';
import type {
  ExtensionAPI,
  ExtensionContext,
} from '@earendil-works/pi-coding-agent';
import {dots, type DotsOptions} from '@joshrwolf/pi-kit/dots';
import {Type, type Static, type TSchema} from 'typebox';
import {Check} from 'typebox/value';
import {TurnMessages} from '../../src/host/delivery.ts';

// Review submission notices: tell the session that started a review (its
// owner) when the user submits a batch of comments in it, so it does not poll.
// The user's drafts are private until submitted, so a submission is the one
// event: one notice per batch, with its summary and every published comment.
//
// The notices themselves are the receipts. Each carries, per review, how many
// submissions it covered; the current branch's notices say what this session
// was told, so reload and resume neither repeat a notice nor lose a submission
// that arrived while Pi was closed. /tree to a branch without a receipt tells
// that branch again. Coverage of a notice not yet on the branch is held in
// memory until the session is idle with nothing held.
//
// A notice waits for the session to be idle and never restarts a run the user
// cancelled; see TurnMessages.
//
// Discovery: on session start, on /tree, after every settled run (the run that
// started a review has ended by then), and whenever a watched review
// directory changes. A run-settle trigger costs one `dots review list`, which
// reads small files; a timer would spend the same process on idle sessions.

export const NOTICE = 'dots.review.notice';
/** Window that batches a burst of changes into one notice. */
export const SETTLE_MS = 750;
const EXCERPT = 300;

// A whole-file anchor has a path alone.
const AnchorSchema = Type.Object({
  path: Type.String(),
  side: Type.Optional(Type.String()),
  start: Type.Optional(Type.Integer()),
  end: Type.Optional(Type.Integer()),
});
const WorktreeSchema = Type.Object({
  path: Type.String(),
  branch: Type.Union([Type.String(), Type.Null()]),
});
const ListSchema = Type.Object({
  reviews: Type.Array(
    Type.Object({
      id: Type.String(),
      file: Type.String(),
    }),
  ),
});
const ShowSchema = Type.Object({
  review: Type.Object({
    id: Type.String(),
    worktree: WorktreeSchema,
    threads: Type.Array(
      Type.Object({
        id: Type.String(),
        anchor: Type.Union([AnchorSchema, Type.Null()]),
        comments: Type.Array(
          Type.Object({
            author: Type.String(),
            body: Type.Union([Type.String(), Type.Null()]),
          }),
        ),
      }),
    ),
    submissions: Type.Array(
      Type.Object({
        body: Type.Union([Type.String(), Type.Null()]),
        comments: Type.Array(
          Type.Object({thread: Type.String(), comment: Type.Integer()}),
        ),
      }),
    ),
  }),
});
type Shown = Static<typeof ShowSchema>['review'];

/** Submissions covered per review id. */
type Marks = Map<string, number>;

export interface NoticeDetails {
  /** Per review id, how many of its submissions this notice covers. */
  marks: Record<string, number>;
}

export interface Watcher {
  close(): void;
}

export interface ReviewHost {
  /** The `dots` JSON client. */
  dots?: (args: readonly string[], options: DotsOptions) => Promise<unknown>;
  /** Watch a review directory; `changed` runs on any change in it. */
  watch?: (directory: string, changed: () => void) => Watcher;
  settleMs?: number;
}

function decode<T extends TSchema>(
  schema: T,
  value: unknown,
  what: string,
): Static<T> {
  if (!Check(schema, value)) throw new Error(`invalid dots ${what} response`);
  return value;
}

function watchDirectory(directory: string, changed: () => void): Watcher {
  const watcher = watch(directory, {persistent: false}, () => changed());
  // A removed worktree takes its review directory with it; rediscovery
  // replaces the watcher.
  watcher.on('error', () => watcher.close());
  return watcher;
}

/** The receipts on `branch`: the highest coverage any notice recorded. */
export function receipts(branch: readonly unknown[]): Marks {
  const marks: Marks = new Map();
  for (const entry of branch) {
    if (!entry || typeof entry !== 'object') continue;
    const record = entry as {
      type?: unknown;
      customType?: unknown;
      details?: {marks?: unknown};
    };
    if (record.type !== 'custom_message' || record.customType !== NOTICE)
      continue;
    const covered = record.details?.marks;
    if (!covered || typeof covered !== 'object') continue;
    for (const [key, count] of Object.entries(covered))
      if (Number.isSafeInteger(count))
        marks.set(key, Math.max(marks.get(key) ?? 0, count as number));
  }
  return marks;
}

interface Comment {
  thread: string;
  location: string;
  opened: boolean;
  body: string | null;
}

interface Batch {
  summary: string | null;
  comments: Comment[];
}

function excerpt(body: string): string {
  const flat = body.replace(/\s+/g, ' ').trim();
  return flat.length > EXCERPT ? `${flat.slice(0, EXCERPT - 1)}…` : flat;
}

function location(anchor: Shown['threads'][number]['anchor']): string {
  if (!anchor) return 'review-level';
  if (anchor.start === undefined || anchor.end === undefined)
    return `${anchor.path} (whole file)`;
  const lines =
    anchor.start === anchor.end
      ? `${anchor.start}`
      : `${anchor.start}-${anchor.end}`;
  return `${anchor.path}:${lines}${anchor.side === 'old' ? ' (old side)' : ''}`;
}

/** The submissions in `review` past what `told` covers, and the new coverage. */
export function submissions(
  review: Shown,
  told: Marks,
): {batches: Batch[]; marks: Record<string, number>} {
  const from = told.get(review.id) ?? 0;
  const batches = review.submissions.slice(from).map(submission => ({
    summary: submission.body,
    comments: submission.comments.map(({thread: id, comment: index}) => {
      const thread = review.threads.find(thread => thread.id === id);
      return {
        thread: id,
        location: location(thread?.anchor ?? null),
        opened: index === 0,
        body: thread?.comments[index]?.body ?? null,
      };
    }),
  }));
  const marks: Record<string, number> =
    batches.length > 0 ? {[review.id]: review.submissions.length} : {};
  return {batches, marks};
}

export function notice(
  reviews: readonly {review: Shown; batches: readonly Batch[]}[],
): string {
  const sections = reviews.flatMap(({review, batches}) => {
    const {path: worktree, branch} = review.worktree;
    return batches.map(batch => {
      const lines = batch.comments.map(
        comment =>
          `- ${comment.thread} · ${comment.location} · ${comment.opened ? 'new thread' : 'reply'}${comment.body ? `: ${excerpt(comment.body)}` : ''}`,
      );
      const threads = [
        ...new Set(batch.comments.map(comment => comment.thread)),
      ].join(', ');
      return [
        `${review.id} in ${worktree}${branch ? ` (${branch})` : ''}`,
        ...(batch.summary ? [`Summary: ${excerpt(batch.summary)}`] : []),
        ...lines,
        `Read: dots review show ${review.id}${threads ? ` (${threads})` : ''}`,
      ].join('\n');
    });
  });
  return [
    'The user submitted review comments in a review this session started.',
    ...sections,
    'Decide whether to answer, fix, or resume the worker that owns the change; then reply on each thread with the outcome (dots review reply ID THREAD --author <you> --body ...). Only the user changes the status of threads the user started.',
  ].join('\n\n');
}

export default function registerReview(
  pi: ExtensionAPI,
  host: ReviewHost = {},
): void {
  const run = host.dots ?? dots;
  const watchOn = host.watch ?? watchDirectory;
  const settleMs = host.settleMs ?? SETTLE_MS;

  let context: ExtensionContext | undefined;
  const delivery = new TurnMessages(pi);
  let lifetime = new AbortController();
  const watchers = new Map<string, Watcher>();
  /** Coverage of notices sent but not yet on the branch. */
  let queued: Marks = new Map();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let syncing: Promise<void> | undefined;
  let again = false;
  let reported: string | undefined;

  const schedule = (delay = settleMs) => {
    if (!context || timer) return;
    timer = setTimeout(() => {
      timer = undefined;
      sync().catch(report);
    }, delay);
  };

  const report = (error: unknown) => {
    const message = String(error instanceof Error ? error.message : error);
    // A session outside a checkout has no reviews to watch.
    if (/not a git repository/i.test(message) || message === reported) return;
    reported = message;
    const text = `Review notices unavailable: ${message.slice(0, 300)}`;
    if (context?.hasUI) context.ui.notify(text, 'warning');
    else console.error(text);
  };

  const rewatch = (directories: ReadonlySet<string>) => {
    for (const [directory, watcher] of watchers)
      if (!directories.has(directory)) {
        watcher.close();
        watchers.delete(directory);
      }
    for (const directory of directories)
      if (!watchers.has(directory)) {
        try {
          watchers.set(
            directory,
            watchOn(directory, () => schedule()),
          );
        } catch (error) {
          report(error);
        }
      }
  };

  const told = (ctx: ExtensionContext): Marks => {
    // Idle with nothing held: every sent notice is on the branch.
    if (ctx.isIdle() && !delivery.holding()) queued = new Map();
    const marks = receipts(ctx.sessionManager.getBranch());
    for (const [key, count] of queued)
      marks.set(key, Math.max(marks.get(key) ?? 0, count));
    return marks;
  };

  const once = async () => {
    const ctx = context;
    if (!ctx) return;
    const signal = lifetime.signal;
    const options: DotsOptions = {cwd: ctx.cwd, directory: ctx.cwd, signal};
    // The identity dots records for a review started in this session's shell.
    const owner = `pi-${ctx.sessionManager.getSessionId()}`;
    const listed = decode(
      ListSchema,
      await run(['review', 'list', '--all', '--owner', owner], options),
      'review list',
    ).reviews;
    if (signal.aborted || context !== ctx) return;
    rewatch(new Set(listed.map(review => path.dirname(review.file))));
    const known = told(ctx);
    const found: {review: Shown; batches: Batch[]}[] = [];
    const marks: Record<string, number> = {};
    for (const {id} of listed) {
      let review: Shown;
      try {
        review = decode(
          ShowSchema,
          await run(['review', 'show', id], options),
          'review show',
        ).review;
      } catch (error) {
        if (signal.aborted) return;
        // Removed since it was listed: nothing to tell.
        if (
          !/no review r-[0-9a-f]+ in this repository/.test(
            error instanceof Error ? error.message : '',
          )
        )
          report(error);
        continue;
      }
      if (signal.aborted || context !== ctx) return;
      const delta = submissions(review, known);
      if (delta.batches.length === 0) continue;
      found.push({review, batches: delta.batches});
      Object.assign(marks, delta.marks);
    }
    if (found.length === 0 || signal.aborted || context !== ctx) return;
    for (const [key, count] of Object.entries(marks)) queued.set(key, count);
    const details: NoticeDetails = {marks};
    const content = notice(found);
    delivery.send(() => ({
      customType: NOTICE,
      content,
      display: true,
      details,
    }));
  };

  const sync = (): Promise<void> => {
    if (syncing) {
      again = true;
      return syncing;
    }
    syncing = (async () => {
      do {
        again = false;
        try {
          await once();
        } catch (error) {
          if (!lifetime.signal.aborted) report(error);
        }
      } while (again && !lifetime.signal.aborted);
    })().finally(() => {
      syncing = undefined;
    });
    return syncing;
  };

  const close = () => {
    lifetime.abort();
    if (timer) clearTimeout(timer);
    timer = undefined;
    for (const watcher of watchers.values()) watcher.close();
    watchers.clear();
    delivery.close();
    context = undefined;
  };

  pi.on('session_start', (_event, ctx) => {
    close();
    lifetime = new AbortController();
    context = ctx;
    delivery.bind(ctx);
    queued = new Map();
    reported = undefined;
    schedule(0);
  });
  pi.on('session_tree', (_event, ctx) => {
    if (!context) return;
    context = ctx;
    // Receipts are branch-local; queued coverage and held notices belonged to
    // the old leaf. The rescan tells this branch what it lacks.
    delivery.discard();
    queued = new Map();
    schedule(0);
  });
  pi.on('agent_settled', () => schedule());
  pi.on('session_shutdown', async () => {
    const pending = syncing;
    close();
    await pending;
  });
}
