import type {
  ExtensionAPI,
  ToolRenderers,
  ToolRendererResolver,
} from '@earendil-works/pi-coding-agent';
import {createToolRenderers} from '../../src/host/tools.ts';
import {
  createSyntaxService,
  type SyntaxService,
} from '../../src/ui/previews/syntax.ts';
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

export function toolRenderers(
  syntax: SyntaxService,
): ReadonlyMap<string, ToolRenderers> {
  return new Map<string, ToolRenderers>([
    ['codemode', createToolRenderers(codemodePresentation(syntax))],
    ['bash', createToolRenderers(bashPresentation(syntax))],
    ['read', createToolRenderers(readPresentation(syntax))],
    ['edit', createToolRenderers(editPresentation(syntax))],
    ['write', createToolRenderers(writePresentation(syntax))],
  ]);
}

const webTools = [
  'fetch_content',
  'web_search',
  'get_search_content',
  'source_check',
];

/** The workbench and live registration share this exact resolution path. */
export function rendererResolver(syntax: SyntaxService): ToolRendererResolver {
  const owned = toolRenderers(syntax);
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
  // Rows can render before session_start. Syntax loading remains lazy.
  const syntax = createSyntaxService();
  pi.registerToolRenderer(rendererResolver(syntax));
  pi.registerMarkdownTransformer(thinkingMarkdown);
  let releaseEditor = () => {};
  pi.on('session_shutdown', () => {
    releaseEditor();
    syntax.dispose();
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
