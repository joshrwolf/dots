import type {
  ExtensionAPI,
  MessageRenderer,
  ToolRenderers,
  ToolRendererResolver,
} from '@earendil-works/pi-coding-agent';
import {createToolRenderers} from '../../src/host/tools.ts';
import {renderJobMessage} from '../jobs/transcript.ts';
import {RECORD_TYPE as JOB_RECORD} from '../jobs/protocol.ts';
import {renderWake} from '../wake/present.ts';
import {renderEvent} from '../workers/present.ts';
import {codemodePresentation} from '../../src/features/tools/codemode.ts';
import {bashPresentation} from '../../src/features/tools/bash.ts';
import {readPresentation} from '../../src/features/tools/read.ts';
import {editPresentation} from '../../src/features/tools/edit.ts';
import {writePresentation} from '../../src/features/tools/write.ts';
import {activityPresentation} from '../../src/features/tools/activity.ts';
import {installEditor} from '../../src/features/editor.ts';
import {createFooter} from '../../src/features/footer.ts';
import {
  hiddenThinkingLabel,
  thinkingMarkdown,
} from '../../src/features/thinking.ts';

export function toolRenderers(): ReadonlyMap<string, ToolRenderers> {
  return new Map<string, ToolRenderers>([
    ['codemode', createToolRenderers(codemodePresentation)],
    ['bash', createToolRenderers(bashPresentation)],
    ['read', createToolRenderers(readPresentation)],
    ['edit', createToolRenderers(editPresentation)],
    ['write', createToolRenderers(writePresentation)],
  ]);
}

/** The companion's custom message renderers, by custom type. */
export function messageRenderers(): ReadonlyMap<string, MessageRenderer> {
  return new Map<string, MessageRenderer>([
    ['dots.worker.event', renderEvent as MessageRenderer],
    ['dots.wake', renderWake as MessageRenderer],
    [JOB_RECORD, renderJobMessage],
    ['dots-jobs-inspection', renderJobMessage],
  ]);
}

const webTools = [
  'fetch_content',
  'web_search',
  'get_search_content',
  'source_check',
];

/** The workbench and live registration share this exact resolution path. */
export function rendererResolver(): ToolRendererResolver {
  const owned = toolRenderers();
  const other = new Map<string, ToolRenderers>();
  return (name, next) => {
    const selected = owned.get(name);
    if (selected) return selected;
    const downstream = next();
    // Web renderers already supply their own headings and diagnostics. Remove
    // only the host fill; keep their content, expansion and lifecycle intact.
    if (
      webTools.includes(name) &&
      downstream?.renderCall &&
      downstream.renderResult &&
      downstream.renderShell !== 'self'
    )
      return {...downstream, renderShell: 'self'};
    // Third-party custom renderers retain ownership. Only otherwise-unstyled
    // tools get the shared transcript shell.
    if (
      downstream?.renderShell === 'self' ||
      downstream?.renderCall ||
      downstream?.renderResult
    )
      return downstream;
    let renderer = other.get(name);
    if (!renderer) {
      renderer = createToolRenderers(activityPresentation(name));
      other.set(name, renderer);
    }
    return renderer;
  };
}

export default function register(pi: ExtensionAPI): void {
  pi.registerToolRenderer(rendererResolver());
  for (const [type, renderer] of messageRenderers())
    pi.registerMessageRenderer(type, renderer);
  pi.registerMarkdownTransformer(thinkingMarkdown);
  let releaseEditor = () => {};
  pi.on('session_shutdown', () => {
    releaseEditor();
  });
  pi.on('session_start', (_event, ctx) => {
    releaseEditor();
    releaseEditor = installEditor(ctx);
    if (ctx.mode !== 'tui') return;
    ctx.ui.setHiddenThinkingLabel(hiddenThinkingLabel);
    ctx.ui.setFooter((tui, theme, data) =>
      createFooter(
        ctx,
        theme,
        data,
        () => tui.requestRender(),
        () => pi.getSettings(),
      ),
    );
  });
}
