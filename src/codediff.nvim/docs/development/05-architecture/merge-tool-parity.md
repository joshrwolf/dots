# Merge Tool Alignment - VSCode Parity

**Date**: December 2025
**Status**: ✅ RESOLVED

## Overview

This document tracks the parity gaps between our merge tool diff rendering and VSCode's implementation. Our goal is 100% replication of VSCode's merge editor rendering for the incoming (left/:3) and current (right/:2) editors.

## Test Framework

Created comparison test framework to validate merge alignment between VSCode and our Lua implementation.

### Files Created

1. **`vscode-merge.mjs`** (project root) - Bundled VSCode merge alignment algorithm
   - Built from VSCode source using `scripts/build-vscode-merge.sh`
   - Uses VSCode's actual `MappingAlignment.compute()` and `getAlignments()`
   - Guarantees 100% identical algorithm to VSCode

2. **`scripts/build-vscode-merge.sh`** - Build script for vscode-merge.mjs
   - Sparse clones VSCode repository
   - Imports from `mergeEditor/browser/model/mapping.ts`, `view/lineAlignment.ts`
   - Uses esbuild to bundle into standalone .mjs

3. **`scripts/merge_alignment_cli.lua`** - Lua CLI tool for our implementation
   - Uses `merge_alignment.compute_merge_fillers_and_conflicts()`
   - Outputs compatible JSON format

4. **`scripts/test_merge_comparison.sh`** - Comparison test script
   - Runs both implementations on same input files
   - Compares diff results, fillers, and alignments
   - Reports differences

### Usage

```bash
# With explicit files
./scripts/test_merge_comparison.sh <base> <input1> <input2>

# Auto-extract from git merge conflict
cd ~/vscode-merge-test
/path/to/scripts/test_merge_comparison.sh
```

## Gap Analysis

Tested with `~/vscode-merge-test` merge conflict (app.py, 244 base lines):

| Metric | VSCode | Lua | Status |
|--------|--------|-----|--------|
| Diff base→input1 | 51 changes | 51 changes | ✅ Match |
| Diff base→input2 | 47 changes | 47 changes | ✅ Match |
| Left fillers count | 13 | 12 | ❌ Differ |
| Right fillers count | 15 | 15 | ⚠️ Positions differ |

### Gap 1: Missing Filler at Line 145
- VSCode produces a filler at `after_line: 145, count: 1`
- Lua implementation misses this filler

### Gap 2: Filler Position Differences
- VSCode: `after_line: 108` vs Lua: `after_line: 103`
- VSCode: `after_line: 281` vs Lua: `after_line: 282`

### Gap 3: Filler Count Difference
- Right filler at line 148: VSCode count=3, Lua count=2

### Root Cause Analysis

The differences stem from how `getAlignments()` processes common equal range mappings:

1. **VSCode's approach**: Uses `toEqualRangeMappings()` with character-level RangeMappings from `innerChanges`
2. **Lua's approach**: Uses line-level inner_changes without full character position information

The key issue is that VSCode's `getAlignments()` operates on character positions (`Position` objects with line+column), while our Lua port may be simplifying to line-only positions.

## Resolution

All parity gaps have been fixed. The comparison test now shows identical filler output between VSCode and our Lua implementation.

### Key Fixes Applied

1. **Event Sort Order in `split_up_common_equal_range_mappings`** (Root cause of most gaps)
   - End events must be processed before start events at the same position
   - This ensures continuous coverage when equal ranges are adjacent
   - Without this fix, we produced "half syncs" instead of "full syncs"

2. **Output Range Extension in `compute_mapping_alignments`**
   - When multiple changes from different inputs are grouped into one alignment
   - Must extend output ranges to cover the full base range using VSCode's `extendInputRange` logic
   - Calculates proper start/end deltas from joined mapping to full base range

### Verification

Run the comparison test to verify parity:
```bash
./scripts/test_merge_comparison.sh  # Auto-detect from ~/vscode-merge-test
./scripts/test_merge_comparison.sh <conflict_file>  # Single file
./scripts/test_merge_comparison.sh <base> <input1> <input2>  # Three files
```

Expected output:
```
Comparing fillers (normalized):
✓ Fillers are IDENTICAL
```

### Previously Identified Gaps (All Resolved)

#### Gap 1: Event Processing Order ✅ FIXED
When equal ranges are adjacent (one ends where another starts at the same position), end events must be processed before start events to maintain continuous coverage.

#### Gap 2: Output Range Extension ✅ FIXED
When grouping changes, output ranges must be extended using the `extendInputRange` pattern to cover the full merged base range.

#### Gap 3-6: Various Edge Cases ✅ FIXED
All other gaps were symptoms of the above two root causes and were resolved by the fixes.

## References

- VSCode `lineAlignment.ts`: Core alignment algorithm
- VSCode `viewZones.ts`: Filler line insertion
- VSCode `inputCodeEditorView.ts`: Decoration application
- VSCode `modifiedBaseRange.ts`: Conflict detection
- VSCode `mapping.ts`: MappingAlignment.compute()
