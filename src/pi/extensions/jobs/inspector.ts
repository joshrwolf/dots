import type {KeybindingsManager, Theme} from '@earendil-works/pi-coding-agent';
import {Inspector, OutputView} from '../../src/index.ts';
import {inspectCard, type JobView} from './present.ts';

/** Inspection never reads artifacts or grants process control. */
export class JobInspector extends Inspector {
  constructor(
    model: JobView,
    theme: Theme,
    bindings: Pick<KeybindingsManager, 'matches'>,
    close: () => void,
    redraw: () => void,
    height?: () => number,
  ) {
    super(bindings, close, redraw, height);
    this.update(model, theme);
  }
  update(model: JobView | undefined, theme: Theme): void {
    this.setContent(
      {
        title: model ? `Job · ${model.subject}` : 'Job inspection',
        detail: model
          ? [
              model.state,
              model.root !== 'root outcome unobserved' ? model.root : '',
            ]
              .filter(Boolean)
              .join(' · ')
          : 'unavailable',
        body: model
          ? inspectCard(model, theme)
          : new OutputView('Job is no longer retained in this history.'),
      },
      theme,
    );
  }
}
