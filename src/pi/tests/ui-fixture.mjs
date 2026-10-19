// Legacy behavioral fixtures consume the new public library, not old entrypoints.
import { after } from "node:test";
import { createSyntaxService, SourceView as Source, DiffView as Diff } from "../src/index.ts";
import { toolRenderers } from "../extensions/ui/index.ts";
const syntax = createSyntaxService();
after(() => syntax.dispose());
export const highlight = (source, path, appearance) => syntax.highlight(source, path, appearance);
export class SourceView extends Source {
  constructor(source, path, theme, notify = () => {}, start = 1) { super({ source, path, theme, syntax, invalidate: notify, startLine: start }); }
}
export class DiffView extends Diff {
  constructor(patch, path, theme, notify = () => {}) { super({ patch, path, theme, syntax, invalidate: notify }); }
}
export const editRenderers = () => toolRenderers(syntax).get("edit");
export const writeRenderers = () => toolRenderers(syntax).get("write");
export const compactRenderers = (name) => toolRenderers(syntax).get(name);
