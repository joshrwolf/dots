import type { AgentToolResult, Theme, ToolRenderers, ToolRendererResolver } from "@earendil-works/pi-coding-agent";
import type { Component } from "@earendil-works/pi-tui";

type HostContext = Parameters<NonNullable<ToolRenderers["renderCall"]>>[2];
type Context = Omit<HostContext, "args" | "state"> & { args: unknown; state: object };
export type ToolObservation =
  | { readonly slot: "call"; readonly cwd?: string; readonly args: unknown; readonly argsComplete: boolean; readonly executionStarted: boolean }
  | { readonly slot: "result"; readonly cwd?: string; readonly args: unknown; readonly content: Readonly<AgentToolResult<unknown>["content"]>; readonly details: unknown; readonly partial: boolean; readonly error: boolean; readonly durationMs: number | undefined; readonly showImages: boolean };

export interface PresentationView<Model> extends Component {
  update(model: Model, theme: Theme, expanded: boolean): void;
}
export interface ToolPresentation<Model> {
  readonly shell: "self" | "default";
  readonly project: (observation: ToolObservation) => Model;
  readonly create: (invalidate: () => void) => PresentationView<Model>;
}

/** Reusable directly in a custom tool definition. No tool registration/execution. */
export function createToolRenderers<Model>(presentation: ToolPresentation<Model>): ToolRenderers {
  type Row = { view: PresentationView<Model>; resultSeen: boolean; padding: number; notify: () => void; call: Component; result: Component };
  // The host's state object gives identity without extending its erased `any`
  // or retaining rows in a strong global registry. Row ownership never switches.
  const rows = new WeakMap<object, Row>();
  function get(context: Context): Row {
    const existing = rows.get(context.state);
    if (existing) { existing.notify = context.invalidate; existing.padding = context.outputPad; return existing; }
    let notify = context.invalidate;
    const view = presentation.create(() => notify());
    const row: Row = {
      view, resultSeen: false, padding: context.outputPad,
      get notify() { return notify; }, set notify(value) { notify = value; },
      call: { render: (width) => row.resultSeen ? [] : render(width), invalidate: () => view.invalidate() },
      result: { render, invalidate: () => view.invalidate() },
    };
    function render(width: number): string[] {
      const padding = presentation.shell === "self" ? row.padding : 0;
      const room = width - 2 * padding;
      return room > 0 ? view.render(room).map((line) => " ".repeat(padding) + line) : [];
    }
    rows.set(context.state, row);
    return row;
  }
  return {
    renderShell: presentation.shell,
    renderCall(args: unknown, theme, context: Context) {
      const row = get(context);
      row.view.update(presentation.project({ slot: "call", cwd: context.cwd, args, argsComplete: context.argsComplete, executionStarted: context.executionStarted }), theme, context.expanded);
      return row.call;
    },
    renderResult(result, options, theme, context: Context) {
      const row = get(context);
      row.resultSeen = true;
      row.view.update(presentation.project({ slot: "result", cwd: context.cwd, args: context.args, content: result.content, details: result.details, partial: options.isPartial, error: context.isError, durationMs: context.durationMs, showImages: context.showImages }), theme, options.expanded);
      return row.result;
    },
  };
}

/** Delegation only occurs at resolution, never halfway through an owned row. */
export function selectToolRenderers(selected: ReadonlyMap<string, ToolRenderers>): ToolRendererResolver {
  return (name, next) => selected.get(name) ?? next();
}
