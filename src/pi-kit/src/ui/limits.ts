/** Preparation budgets, not terminal widths. Pi owns display-column arithmetic. */
export const displayLimits = {
  sourceCodeUnits: 500_000,
  sourceLines: 10_000,
  patchLines: 10_000,
  previewRows: 1000,
  outputRows: 2000,
  syntaxCodeUnits: 80_000,
  syntaxLineCodeUnits: 2000,
  syntaxHunks: 128,
} as const;
