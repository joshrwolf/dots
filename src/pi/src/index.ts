// Consumer surface: importing the library registers nothing and starts no work.
export { Inspector, type InspectorModel } from "./ui/inspector.ts";
export { activityDecoration, StateGutter, type ActivityState } from "./ui/state.ts";
export { SoftEditor } from "./features/editor.ts";
export { createToolRenderers, selectToolRenderers, type PresentationView, type ToolObservation, type ToolPresentation } from "./host/tools.ts";
export { PriorityRow, fitLabel, type RowModel, type RowLayout, type Segment, type Tone } from "./ui/row.ts";
export { activityHeader, Notices, OutputView, PreviewView, ActivityCard, type ActivityCardModel, type Notice } from "./ui/card.ts";
export { readableForeground } from "./ui/color.ts";
export { DetailView } from "./ui/details.ts";
export { sourceText, labelText, statusText, plainLines, layoutText } from "./ui/text.ts";
export { SourceView, DiffView, type SourceViewOptions, type DiffViewOptions } from "./ui/previews/view.ts";
export { parsePatch, type Patch } from "./ui/previews/patch.ts";
export { createSyntaxService, type SyntaxService, type SyntaxEngine, type SyntaxToken, type Tokens } from "./ui/previews/syntax.ts";
