# VSCode Diff Algorithm Extraction Tool

**Date:** October 28, 2025
**Status:** ✅ Complete

This script extracts and bundles VSCode's diff algorithm into a standalone JavaScript executable.

## Quick Start

```bash
# Generate the standalone diff tool
./scripts/build-vscode-diff.sh

# This creates: vscode-diff.mjs (default name)
# Or specify a custom name:
./scripts/build-vscode-diff.sh my-diff-tool.mjs
```

## Usage

```bash
node vscode-diff.mjs <file1> <file2>
```

## Example

```bash
# Create test files
echo "line1" > test1.txt
echo "line2" > test2.txt

# Run diff
node vscode-diff.mjs test1.txt test2.txt
```

## Requirements

- Node.js (for running the generated tool)
- npm (for esbuild during build process)
- Git (for cloning VSCode repo)

## What It Does

1. Clones VSCode repository (sparse checkout for minimal size)
2. Extracts only the diff algorithm files (~260 source files)
3. Creates a wrapper script for CLI usage
4. Bundles everything into a single ~239KB JavaScript file
5. Cleans up temporary files

## Use Cases

- **Reference implementation** for testing
- **Source of truth** for validation
- **Debugging tool** for comparing outputs
- **Standalone diff** without installing VSCode

## Technical Details

- **Bundle size:** ~239KB
- **Build time:** ~20-30 seconds (includes git clone)
- **Output format:** ESM (requires Node.js with ESM support)
- **Algorithm:** Same as VSCode's DefaultLinesDiffComputer
- **Dependencies:** None (fully bundled)

## Feasibility Study

### Repository Analysis

Located the core diff algorithm in VSCode:
- **Main file:** `src/vs/editor/common/diff/defaultLinesDiffComputer/defaultLinesDiffComputer.ts`
- **Key function:** `computeDiff(originalLines, modifiedLines, options)`
- **Dependencies:** Base utilities and core editor types

### Extraction Process

Used sparse Git checkout to minimize download size:
```bash
git clone --depth 1 --filter=blob:none --sparse https://github.com/microsoft/vscode.git
git sparse-checkout set src/vs/editor/common/diff src/vs/base/common src/vs/editor/common/core
```

### Bundling Strategy

Created a wrapper script and bundled with esbuild:
- **Input:** TypeScript wrapper + VSCode source code
- **Output:** Single ESM JavaScript file (~239KB)
- **Bundler:** esbuild (fast, zero-config)
- **Build time:** ~20-30ms

## Output Format

The tool outputs the exact same format as the C implementation's `print_linesdiff` function, making direct comparison trivial:

```
=================================================================
Diff Tool - Computing differences
=================================================================
Original: test1.txt (5 lines)
Modified: test2.txt (5 lines)
=================================================================

Diff Results:
=================================================================
Number of changes: 2
Hit timeout: no

  Changes: 2 line mapping(s)
    [0] Lines 2-3 -> Lines 2-3 (2 inner changes)
         Inner: L2:C10-L2:C10 -> L2:C10-L2:C19
         Inner: L3:C7-L3:C7 -> L3:C7-L3:C16
    [1] Lines 5-5 -> Lines 5-5 (1 inner change)
         Inner: L5:C6-L5:C7 -> L5:C6-L5:C13

=================================================================
```

### Identical Output Format

The JavaScript tool outputs in **exactly the same format** as the C implementation:

- Same header structure
- Same line mapping format
- Same inner change notation
- Character positions use the same notation (L#:C#)

## Integration

### As Source of Truth

The VSCode JavaScript bundle can serve as a reference implementation:

1. **Validation:** Test our C implementation against VSCode's output
2. **Regression Testing:** Ensure algorithm parity
3. **Feature Comparison:** Verify move detection, character-level diffs, etc.
4. **Debugging:** Compare outputs when results differ

### Usage Pattern

```bash
# Run VSCode diff
node vscode-diff.mjs original.txt modified.txt

# Run our C diff
./build/diff original.txt modified.txt

# Outputs are now directly comparable - same format!
```

### Direct Comparison

Since both tools use the **identical output format**, you can:

1. **Diff the outputs directly:** `diff <(node vscode-diff.mjs f1 f2) <(./build/diff f1 f2)`
2. **Automated testing:** Compare outputs byte-for-byte in test suites
3. **Regression detection:** Any format change indicates a discrepancy
4. **Debugging:** Side-by-side comparison is trivial

## Files Generated

### 1. Build Script
- **Location:** `scripts/build-vscode-diff.sh`
- **Purpose:** Automated extraction and bundling
- **Size:** ~4KB

### 2. Standalone VSCode Diff Tool
- **Example:** `/tmp/test-build/my-vscode-diff.mjs`
- **Size:** 238.9KB
- **Runtime:** Node.js (ESM)
- **Dependencies:** None (fully bundled)

### 3. Our C Diff Tool
- **Location:** `c-diff-core/build/diff`
- **Built via:** `make diff-tool`
- **Size:** Varies by platform
- **Runtime:** Native (no dependencies)

## Recommendations

### For Testing & Validation

1. **Add VSCode reference tests:** Use the JavaScript tool as oracle for test cases
2. **Create comparison suite:** Automate output comparison
3. **Validate edge cases:** Use VSCode's implementation to verify complex scenarios

### For CI/CD

Consider adding a test that:
1. Runs VSCode diff tool
2. Runs our C diff tool
3. Compares algorithmic results
4. Fails if results diverge

## Testing Examples

### Simple Text Change
```bash
# Input files
echo -e "Hello World\nThis is a test file\nLine 3\nLine 4\nLine 5" > test1.txt
echo -e "Hello World\nThis is a modified test file\nLine 3 modified\nLine 4\nLine 6 added" > test2.txt

# VSCode output
node vscode-diff.mjs test1.txt test2.txt

# C output
./build/diff test1.txt test2.txt
```

### Code Move Detection
```bash
# Create files with function reordering
# VSCode detects moves when computeMoves: true
node vscode-diff.mjs code_before.js code_after.js
```

## Conclusion

Extracting VSCode's diff algorithm is not only possible but **remarkably easy**. The automated build script makes it a single command to generate a standalone, dependency-free JavaScript executable that can serve as a perfect source of truth for validating our C implementation.

The ~239KB bundle is small enough to:
- Check into the repo (if desired)
- Include in test fixtures
- Distribute with documentation
- Run in CI/CD pipelines

## See Also

- [VSCode Source](https://github.com/microsoft/vscode/blob/main/src/vs/editor/common/diff/defaultLinesDiffComputer/defaultLinesDiffComputer.ts) - Original algorithm
