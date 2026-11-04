// Internal barrel for extensions: importing it registers nothing and starts no work.
export {
  Panel,
  type PanelModel,
  PriorityRow,
  fitLabel,
  type RowModel,
  type RowLayout,
  type Segment,
  type Tone,
  readableForeground,
  sourceText,
  labelText,
  statusText,
  plainLines,
  layoutText,
} from '@joshrwolf/pi-kit/ui';
export {Inspector, type InspectorModel} from './ui/inspector.ts';
export {
  activityDecoration,
  StateGutter,
  type ActivityState,
} from './ui/state.ts';
export {
  contextMeter,
  contextSegments,
  CONTEXT_WARN,
  CONTEXT_FULL,
} from './ui/context.ts';
export {SoftEditor} from './features/editor.ts';
export {
  createToolRenderers,
  type PresentationOptions,
  type PresentationView,
  type ToolObservation,
  type ToolPresentation,
} from './host/tools.ts';
export {
  activityHeader,
  expandCue,
  Notices,
  OutputView,
  PreviewView,
  ActivityCard,
  type ActivityCardModel,
  type Notice,
} from './ui/card.ts';
export {DetailView} from './ui/details.ts';
export {
  SourceView,
  DiffView,
  type SourceViewOptions,
} from './ui/previews/view.ts';
