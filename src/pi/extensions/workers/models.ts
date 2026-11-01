import {
  clampThinkingLevel,
  type Api,
  type Model,
  type ModelThinkingLevel,
} from '@earendil-works/pi-ai';
import {
  catalogOf,
  checkThinking,
  findModel,
  isThinkingLevel,
  refOf,
  type CatalogEntry,
  type Check,
} from '@joshrwolf/pi-kit/models';

// The models a spawn may choose from: the session's scoped models
// (`enabledModels`), validated by the shared catalog checks. Nothing here
// names a model, so new releases and changes to `enabledModels` need no code change.

/** What a spawn asks for. Both fields are required so the choice is deliberate. */
export interface ModelChoice {
  readonly model: string;
  readonly thinking: string;
}

export interface ParentModel {
  readonly model?: Model<Api> | undefined;
  readonly thinking?: string | undefined;
}

export type Resolution =
  | {
      readonly ok: true;
      readonly model: string;
      readonly thinking: ModelThinkingLevel;
    }
  | {readonly ok: false; readonly error: string};

/** Scoped models in their configured order; without scoping, only the parent's model. */
export function buildCatalog(
  scoped: readonly {readonly model: Model<Api>}[],
  current: Model<Api> | undefined,
): CatalogEntry[] {
  return catalogOf(
    scoped.length ? scoped.map(s => s.model) : current ? [current] : [],
  );
}

/**
 * Resolve a spawn's choice to a concrete model and thinking level, rejecting
 * anything the catalog or the chosen model does not support. `inherit` uses
 * the parent's value; thinking `default` is Pi's default level clamped to the
 * chosen model, which is what the child would start with on its own.
 */
export function resolveChoice(
  choice: ModelChoice,
  catalog: readonly CatalogEntry[],
  parent: ParentModel,
  defaultThinking: ModelThinkingLevel,
): Resolution {
  const fail = (error: string): Resolution => ({ok: false, error});
  let model: Model<Api>;
  if (choice.model === 'inherit') {
    if (!parent.model)
      return fail(
        'model inherit: this session has no current model; choose one from the catalog',
      );
    model = parent.model;
  } else {
    const found = findModel(choice.model, catalog, ['inherit']);
    if (!found.ok) return fail(found.error);
    model = found.value.model;
  }
  if (choice.thinking === 'default')
    return {
      ok: true,
      model: refOf(model),
      thinking: clampThinkingLevel(model, defaultThinking),
    };
  let checked: Check<ModelThinkingLevel>;
  if (choice.thinking === 'inherit') {
    if (!parent.thinking || !isThinkingLevel(parent.thinking))
      return fail(
        'thinking inherit: this session has no current thinking level; choose default or a level',
      );
    checked = checkThinking(
      model,
      parent.thinking,
      `inherit (${parent.thinking})`,
    );
  } else checked = checkThinking(model, choice.thinking);
  return checked.ok
    ? {ok: true, model: refOf(model), thinking: checked.value}
    : fail(checked.error);
}

const price = (value: number) =>
  Number.isInteger(value)
    ? String(value)
    : String(Number(value.toPrecision(2)));
const tokens = (value: number) =>
  value >= 1_000_000
    ? `${price(value / 1_000_000)}M`
    : `${Math.round(value / 1000)}K`;

/** One line per model: what the agent needs to compare them. */
export function describeCatalog(
  catalog: readonly CatalogEntry[],
  parent: ParentModel,
): string {
  if (!catalog.length)
    return 'No models are scoped; only inherit is available.';
  const current = parent.model ? refOf(parent.model) : undefined;
  const lines = catalog.map(({ref, model, levels}) => {
    const images = model.input.includes('image') ? ' · images' : '';
    const mark = ref === current ? ' (current)' : '';
    return `- ${ref}${mark}: $${price(model.cost.input)}/$${price(model.cost.output)} per M in/out · ${tokens(model.contextWindow)} context${images} · thinking ${levels.join(', ')}`;
  });
  return `Models:\n${lines.join('\n')}`;
}

export const CHOICE_GUIDANCE =
  "Choose the model and thinking level together for the worker's bounded task. Use the available catalog and observed results; price and effort labels alone don't establish capability, and effort levels aren't comparable across models. Weigh reasoning difficulty, uncertainty, and the cost of mistakes against latency and cost. Task length or labels such as review and debugging aren't sufficient reasons for higher effort. Keep the choice proportional to the work.\n\ninherit uses this session's value; thinking default is Pi's default level for the chosen model.";
