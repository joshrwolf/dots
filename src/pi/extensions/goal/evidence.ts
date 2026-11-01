import type {ExtensionContext} from '@earendil-works/pi-coding-agent';
import {matchesKey, VStack} from '@earendil-works/pi-tui';
import {labelText} from '@joshrwolf/pi-kit/ui';
import {
  Inspector,
  inspectorHeight,
  inspectorOverlay,
} from '../../src/ui/inspector.ts';
import {OutputView} from '../../src/ui/card.ts';
import {MarkdownView} from '../../src/ui/markdown.ts';
import type {Evidence, EvidenceRecord, EvidenceSource} from './source.ts';

export const EVIDENCE_HELP =
  '/goal evidence TASK [EVENT|current] [PROJECT]. Read-only; defaults to this checkout’s project. Historical events retain exact selections; inspection never accepts work.';
export function evidenceArgs(words: string[]): {
  id: string;
  seq?: number;
  project?: string;
} {
  const [id, event, project] = words;
  if (
    !id ||
    !/^t-[a-f0-9]+$/.test(id) ||
    words.length > 3 ||
    (event !== undefined &&
      event !== 'current' &&
      (!/^[1-9][0-9]*$/.test(event) || !Number.isSafeInteger(Number(event))))
  )
    throw new Error(EVIDENCE_HELP);
  return {
    id,
    ...(event && event !== 'current' ? {seq: Number(event)} : {}),
    ...(project ? {project} : {}),
  };
}
export function evidenceRecordText(record: EvidenceRecord): string {
  return [
    `Task: ${record.task}`,
    `Project: ${record.project}`,
    `Result event: ${record.seq ?? 'none'} · current at observation: ${record.current ?? 'none'}`,
    'Recorded result and exact selections; not a fresh acceptance or availability guarantee.',
    record.result ?? 'No result recorded.',
    ...record.evidence.map(
      item =>
        `${item.slug} ${item.digest} · ${record.purged.some(p => p.slug === item.slug && p.digest === item.digest) ? 'purged; unavailable' : 'pinned; bytes not revalidated'}`,
    ),
    ...(!record.evidence.length
      ? ['No retained evidence selected. Result-only work is valid.']
      : []),
    `Discovery attachments are separate: dots output list --task ${record.task} --project ${record.project} --json`,
  ].join('\n\n');
}

/** Read-only CLI bytes, rendered with the same stock overlay as goal inspection. */
export async function inspectEvidence(
  ctx: ExtensionContext,
  record: EvidenceRecord,
  source: EvidenceSource,
  signal: AbortSignal,
): Promise<void> {
  const choices = [
    'Result record',
    'Attached outputs (not acceptance)',
    ...record.evidence.map(item => `${item.slug} ${item.digest}`),
  ].map(labelText);
  while (!signal.aborted) {
    const choice = await ctx.ui.select('Recorded task evidence', choices, {
      signal,
    });
    if (choice === undefined || signal.aborted) return;
    if (choice === choices[1]) {
      await inspectAttachments(ctx, record, source, signal);
      continue;
    }
    const selection = record.evidence[choices.indexOf(choice) - 2];
    await inspectDocument(
      ctx,
      record.project,
      source,
      signal,
      selection
        ? `${record.task} · result event ${record.seq}`
        : evidenceRecordText(record),
      selection,
    );
  }
}

async function inspectDocument(
  ctx: ExtensionContext,
  project: string,
  source: EvidenceSource,
  signal: AbortSignal,
  heading: string,
  selection?: Evidence,
): Promise<void> {
  const starts = [1];
  let reading = true;
  while (reading && !signal.aborted) {
    const start = starts.at(-1)!;
    const page = selection
      ? await source.page(ctx.cwd, project, selection, start, signal)
      : undefined;
    if (signal.aborted) return;
    const action = await ctx.ui.custom<'next' | 'back' | 'close'>(
      (tui, theme, bindings, done) => {
        const abort = () => done('close');
        signal.addEventListener('abort', abort, {once: true});
        if (signal.aborted) {
          abort();
          return {render: () => [], invalidate() {}};
        }
        const inspector = new Inspector(
          bindings,
          () => done('close'),
          () => tui.requestRender(),
          () => inspectorHeight(tui.terminal.rows),
        );
        const body =
          page && selection
            ? new VStack([
                new OutputView(
                  `${heading}\n${selection.slug} ${selection.digest}\n${page.path}\nState: ${page.state}${page.state === 'available' ? ' · digest verified for this read' : ''}\nPage starts at line ${start}${page.next === null ? '' : ` · more at ${page.next}`}\n`,
                ),
                page.state === 'available' && page.path.endsWith('.md')
                  ? new MarkdownView(page.text, theme)
                  : new OutputView(page.text),
              ])
            : new OutputView(heading);
        inspector.setContent(
          {
            title: 'Recorded task evidence',
            body,
            hint: `${page?.next ? 'N next page · ' : ''}${starts.length > 1 ? 'B previous page · ' : ''}↑/↓ scroll · Esc/Enter selections`,
          },
          theme,
          true,
        );
        return {
          render: (width: number) => inspector.render(width),
          invalidate: () => inspector.invalidate(),
          handleInput: (data: string) => {
            if (matchesKey(data, 'n') && page?.next) done('next');
            else if (matchesKey(data, 'b') && starts.length > 1) done('back');
            else inspector.handleInput(data);
          },
          dispose: () => signal.removeEventListener('abort', abort),
        };
      },
      inspectorOverlay,
    );
    if (action === 'next' && page?.next) starts.push(page.next);
    else if (action === 'back' && starts.length > 1) starts.pop();
    else reading = false;
  }
}

/** Discovery never chooses an implicit latest revision or changes an outcome. */
async function inspectAttachments(
  ctx: ExtensionContext,
  record: EvidenceRecord,
  source: EvidenceSource,
  signal: AbortSignal,
): Promise<void> {
  let offset = 0;
  while (!signal.aborted) {
    const attached = await source.attached(
      ctx.cwd,
      record.project,
      record.task,
      offset,
      signal,
    );
    if (signal.aborted) return;
    const more = 'More outputs…';
    const choice = await ctx.ui.select(
      'Discovery attachments · not accepted evidence',
      [...attached.slugs, ...(attached.next === null ? [] : [more])],
      {signal},
    );
    if (signal.aborted || choice === undefined) return;
    if (choice === more && attached.next !== null) {
      offset = attached.next;
      continue;
    }
    if (!attached.slugs.includes(choice)) return;
    let revisionOffset = 0;
    while (!signal.aborted) {
      const manifest = await source.manifest(
        ctx.cwd,
        record.project,
        choice,
        revisionOffset,
        signal,
      );
      if (signal.aborted) return;
      const next = 'More revisions…';
      const labels = manifest.revisions.map(
        item => `${item.digest}${item.purged ? ' · purged' : ''}`,
      );
      const selected = await ctx.ui.select(
        `${choice} · choose exact revision; no latest head`,
        [...labels, ...(manifest.next === null ? [] : [next])],
        {signal},
      );
      if (signal.aborted) return;
      if (selected === undefined) break;
      if (selected === next && manifest.next !== null) {
        revisionOffset = manifest.next;
        continue;
      }
      const revision = manifest.revisions[labels.indexOf(selected)];
      if (!revision) break;
      await inspectDocument(
        ctx,
        record.project,
        source,
        signal,
        `Discovery attachment to ${record.task}; not accepted evidence.\nDescription: ${revision.description}\nProvenance (attributed): ${revision.provenance}`,
        revision,
      );
    }
  }
}
