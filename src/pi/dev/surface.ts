import {readFileSync} from 'node:fs';
import type {Theme} from '@earendil-works/pi-coding-agent';
import {backgroundAnsi, parseColor} from '@earendil-works/pi-tui';
import {paintSurfaceLine} from '@joshrwolf/pi-kit/ui';

// Development-only palette data, loaded once, never during rendering. The preview
// page is distinct from userMessageBg; the latter belongs only to prompts.
export function pageBackground(value: unknown) {
  if (
    !value ||
    typeof value !== 'object' ||
    !('export' in value) ||
    !value.export ||
    typeof value.export !== 'object' ||
    !('pageBg' in value.export) ||
    typeof value.export.pageBg !== 'string'
  )
    throw new Error('Preview theme requires export.pageBg');
  return parseColor(value.export.pageBg);
}
function loadPage(appearance: 'light' | 'dark') {
  const palette: unknown = JSON.parse(
    readFileSync(
      new URL(`../themes/dots-${appearance}.json`, import.meta.url),
      'utf8',
    ),
  );
  return pageBackground(palette);
}
const pages = {light: loadPage('light'), dark: loadPage('dark')};

/** Rebind terminal-default SGR resets to the preview page, while preserving
 * explicit child colors (prompt panels, selections, diffs, syntax). This only
 * decorates existing lines; Pi still owns terminal rendering and composition. */
export function paintPreviewLine(
  line: string,
  width: number,
  theme: Theme,
): string {
  return paintSurfaceLine(line, width, {
    fg: theme.getFgAnsi('text'),
    bg: backgroundAnsi(pages[theme.appearance], theme.getColorMode()),
  });
}
