# C Diff Algorithm Development

Development logs for the C core diff algorithm — porting VSCode's TypeScript diff engine to C with full parity.

## Reading Order

1. **[Implementation Plan](implementation-plan.md)** — Overall architecture, pipeline design, all 4 steps
2. **[Step 1: Myers Algorithm](step1-myers-devlog.md)** — Forward O(ND) Myers from prototype to parity
3. **[Steps 2-3: Line Optimization](step2-step3-optimization-devlog.md)** — Heuristic optimization pipeline
4. **[Step 4: Character Refinement](step4-char-refinement-devlog.md)** — Character-level diff for inline highlighting
5. **[Parity Evaluation Journey](parity-evaluation-journey.md)** — The full story of chasing VSCode parity through 3 evaluations and dozens of fixes
6. **[UTF-8 & VSCode Parity](utf8-and-vscode-parity.md)** — Encoding differences between C and JavaScript, and all fixes applied
7. **[Post-Timeout Parity Checklist](post-timeout-parity-checklist.md)** — Remaining work items for full end-to-end parity
