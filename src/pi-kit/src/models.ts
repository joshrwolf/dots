import {
  getSupportedThinkingLevels,
  type Api,
  type Model,
  type ModelThinkingLevel,
} from '@earendil-works/pi-ai';

// Model choices validated against a catalog that callers read from Pi at
// runtime. Nothing here names a model, so new releases and catalog changes
// need no code change. Errors list the valid choices.

export const THINKING_LEVELS = [
  'off',
  'minimal',
  'low',
  'medium',
  'high',
  'xhigh',
  'max',
] as const satisfies readonly ModelThinkingLevel[];

export interface CatalogEntry {
  /** `provider/id`, the value a choice names. */
  readonly ref: string;
  readonly model: Model<Api>;
  readonly levels: readonly ModelThinkingLevel[];
}

export type Check<T> =
  | {readonly ok: true; readonly value: T}
  | {readonly ok: false; readonly error: string};

export const refOf = (model: Pick<Model<Api>, 'provider' | 'id'>): string =>
  `${model.provider}/${model.id}`;

export const isThinkingLevel = (value: string): value is ModelThinkingLevel =>
  (THINKING_LEVELS as readonly string[]).includes(value);

/** Catalog entries in the given order, keeping the first of any repeated `provider/id`. */
export function catalogOf(models: Iterable<Model<Api>>): CatalogEntry[] {
  const seen = new Set<string>();
  const entries: CatalogEntry[] = [];
  for (const model of models) {
    const ref = refOf(model);
    if (seen.has(ref)) continue;
    seen.add(ref);
    entries.push({ref, model, levels: getSupportedThinkingLevels(model)});
  }
  return entries;
}

/**
 * Find `provider/id` in the catalog. `also` names caller-specific choices that
 * are valid besides the catalog, such as `inherit`, so the error lists them too.
 */
export function findModel(
  ref: string,
  catalog: readonly CatalogEntry[],
  also: readonly string[] = [],
): Check<CatalogEntry> {
  const found = catalog.find(entry => entry.ref === ref);
  if (found) return {ok: true, value: found};
  const prefix = also.length ? `${also.join(', ')} or ` : '';
  return {
    ok: false,
    error: `model ${ref} is not available; choose ${prefix}one of: ${catalog.map(entry => entry.ref).join(', ') || '(none)'}`,
  };
}

/** Check that `model` supports a thinking level. `label` names the choice in the error when it differs from the level. */
export function checkThinking(
  model: Model<Api>,
  level: string,
  label = level,
): Check<ModelThinkingLevel> {
  const levels = getSupportedThinkingLevels(model);
  if (isThinkingLevel(level) && levels.includes(level))
    return {ok: true, value: level};
  return {
    ok: false,
    error: `thinking ${label} is unsupported: ${refOf(model)} supports thinking ${levels.join(', ')}`,
  };
}
