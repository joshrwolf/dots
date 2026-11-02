#!/usr/bin/env bash
# Script to extract and bundle VSCode's merge alignment algorithm into a standalone executable
# This ensures we use the EXACT same algorithm as VSCode for comparison testing

set -e

# Remember where we started
START_DIR="$(pwd)"

WORK_DIR=$(mktemp -d)
trap "rm -rf $WORK_DIR" EXIT

echo "Working directory: $WORK_DIR"
cd "$WORK_DIR"

echo "Cloning VSCode repository (sparse checkout)..."
git clone --depth 1 --filter=blob:none --sparse https://github.com/microsoft/vscode.git

cd vscode
# Need diff algorithm + merge editor code
git sparse-checkout set \
    src/vs/editor/common/diff \
    src/vs/base/common \
    src/vs/editor/common/core \
    src/vs/workbench/contrib/mergeEditor/browser/model \
    src/vs/workbench/contrib/mergeEditor/browser/view

cd "$WORK_DIR"

echo "Creating wrapper script..."
cat > vscode-merge-wrapper.ts << 'EOF'
#!/usr/bin/env node
import { readFileSync } from 'fs';
import { DefaultLinesDiffComputer } from './vscode/src/vs/editor/common/diff/defaultLinesDiffComputer/defaultLinesDiffComputer.js';
import { MappingAlignment, DetailedLineRangeMapping, LineRangeMapping, RangeMapping } from './vscode/src/vs/workbench/contrib/mergeEditor/browser/model/mapping.js';
import { MergeEditorLineRange } from './vscode/src/vs/workbench/contrib/mergeEditor/browser/model/lineRange.js';
import { getAlignments, LineAlignment } from './vscode/src/vs/workbench/contrib/mergeEditor/browser/view/lineAlignment.js';
import { Range } from './vscode/src/vs/editor/common/core/range.js';

// Minimal ITextModel mock - only needs line count for our purposes
class MockTextModel {
    constructor(private lines: string[]) {}
    getLineCount(): number { return this.lines.length; }
    getLineContent(lineNumber: number): string { return this.lines[lineNumber - 1] || ''; }
}

function main() {
    const args = process.argv.slice(2);
    
    if (args.length < 3) {
        console.error('Usage: node vscode-merge.mjs <base> <input1> <input2>');
        console.error('  base   - base version file');
        console.error('  input1 - left/current version file');
        console.error('  input2 - right/incoming version file');
        process.exit(1);
    }

    const [basePath, input1Path, input2Path] = args;

    const baseContent = readFileSync(basePath, 'utf-8');
    const input1Content = readFileSync(input1Path, 'utf-8');
    const input2Content = readFileSync(input2Path, 'utf-8');

    const baseLines = baseContent.split('\n');
    const input1Lines = input1Content.split('\n');
    const input2Lines = input2Content.split('\n');

    console.error(`Base: ${basePath} (${baseLines.length} lines)`);
    console.error(`Input1 (current): ${input1Path} (${input1Lines.length} lines)`);
    console.error(`Input2 (incoming): ${input2Path} (${input2Lines.length} lines)`);

    // Create mock text models
    const baseModel = new MockTextModel(baseLines);
    const input1Model = new MockTextModel(input1Lines);
    const input2Model = new MockTextModel(input2Lines);

    // Compute diffs using VSCode's diff algorithm
    const diffComputer = new DefaultLinesDiffComputer();
    
    const diff1Result = diffComputer.computeDiff(baseLines, input1Lines, {
        ignoreTrimWhitespace: false,
        maxComputationTimeMs: 5000,
        computeMoves: false,
        extendToSubwords: false,
    });
    
    const diff2Result = diffComputer.computeDiff(baseLines, input2Lines, {
        ignoreTrimWhitespace: false,
        maxComputationTimeMs: 5000,
        computeMoves: false,
        extendToSubwords: false,
    });

    console.error(`Diff base->input1: ${diff1Result.changes.length} changes`);
    console.error(`Diff base->input2: ${diff2Result.changes.length} changes`);

    // Convert diff results to DetailedLineRangeMapping format
    // This is what VSCode's ModifiedBaseRange.fromDiffs expects
    const diffs1: DetailedLineRangeMapping[] = diff1Result.changes.map(c => {
        const inputRange = MergeEditorLineRange.fromLineNumbers(
            c.original.startLineNumber,
            c.original.endLineNumberExclusive
        );
        const outputRange = MergeEditorLineRange.fromLineNumbers(
            c.modified.startLineNumber,
            c.modified.endLineNumberExclusive
        );
        
        // Convert inner changes to RangeMapping
        const rangeMappings = (c.innerChanges || []).map(inner => 
            new RangeMapping(inner.originalRange, inner.modifiedRange)
        );
        
        return new DetailedLineRangeMapping(
            inputRange,
            baseModel as any,
            outputRange,
            input1Model as any,
            rangeMappings
        );
    });
    
    const diffs2: DetailedLineRangeMapping[] = diff2Result.changes.map(c => {
        const inputRange = MergeEditorLineRange.fromLineNumbers(
            c.original.startLineNumber,
            c.original.endLineNumberExclusive
        );
        const outputRange = MergeEditorLineRange.fromLineNumbers(
            c.modified.startLineNumber,
            c.modified.endLineNumberExclusive
        );
        
        const rangeMappings = (c.innerChanges || []).map(inner => 
            new RangeMapping(inner.originalRange, inner.modifiedRange)
        );
        
        return new DetailedLineRangeMapping(
            inputRange,
            baseModel as any,
            outputRange,
            input2Model as any,
            rangeMappings
        );
    });

    // Use VSCode's MappingAlignment.compute - this is the core algorithm
    const mappingAlignments = MappingAlignment.compute(diffs1, diffs2);
    
    console.error(`Mapping alignments: ${mappingAlignments.length}`);

    // Build output structure
    const output: any = {
        files: {
            base: { path: basePath, lines: baseLines.length },
            input1: { path: input1Path, lines: input1Lines.length },
            input2: { path: input2Path, lines: input2Lines.length },
        },
        diffs: {
            base_to_input1: diff1Result.changes.map(c => ({
                original: { start_line: c.original.startLineNumber, end_line: c.original.endLineNumberExclusive },
                modified: { start_line: c.modified.startLineNumber, end_line: c.modified.endLineNumberExclusive },
                inner_changes_count: (c.innerChanges || []).length,
            })),
            base_to_input2: diff2Result.changes.map(c => ({
                original: { start_line: c.original.startLineNumber, end_line: c.original.endLineNumberExclusive },
                modified: { start_line: c.modified.startLineNumber, end_line: c.modified.endLineNumberExclusive },
                inner_changes_count: (c.innerChanges || []).length,
            })),
        },
        mapping_alignments: [] as any[],
        fillers: {
            left_fillers: [] as any[],
            right_fillers: [] as any[],
        },
    };

    let leftTotal = 0;
    let rightTotal = 0;

    for (const ma of mappingAlignments) {
        const isConflicting = ma.output1LineMappings.length > 0 && ma.output2LineMappings.length > 0;

        // Create a minimal ModifiedBaseRange-like object for getAlignments
        // getAlignments expects: baseRange, input1Range, input2Range, input1Diffs, input2Diffs
        const modifiedBaseRangeLike = {
            baseRange: ma.inputRange,
            input1Range: ma.output1Range,
            input2Range: ma.output2Range,
            input1Diffs: ma.output1LineMappings,
            input2Diffs: ma.output2LineMappings,
        };

        // Get line alignments using VSCode's getAlignments
        const alignments = getAlignments(modifiedBaseRangeLike as any);

        output.mapping_alignments.push({
            base_range: { start: ma.inputRange.startLineNumber, end: ma.inputRange.endLineNumberExclusive },
            input1_range: { start: ma.output1Range.startLineNumber, end: ma.output1Range.endLineNumberExclusive },
            input2_range: { start: ma.output2Range.startLineNumber, end: ma.output2Range.endLineNumberExclusive },
            is_conflicting: isConflicting,
            alignments: alignments.map((a: LineAlignment) => ({ input1: a[0], base: a[1], input2: a[2] })),
        });

        // Calculate fillers from alignments (same logic as VSCode's viewZones.ts)
        for (const a of alignments) {
            if (a[0] !== undefined && a[2] !== undefined) {
                const leftAdj = a[0] + leftTotal;
                const rightAdj = a[2] + rightTotal;
                const mx = Math.max(leftAdj, rightAdj);

                if (mx - leftAdj > 0) {
                    output.fillers.left_fillers.push({ after_line: a[0] - 1, count: mx - leftAdj });
                    leftTotal += mx - leftAdj;
                }
                if (mx - rightAdj > 0) {
                    output.fillers.right_fillers.push({ after_line: a[2] - 1, count: mx - rightAdj });
                    rightTotal += mx - rightAdj;
                }
            }
        }
    }

    // Output JSON to stdout
    console.log(JSON.stringify(output, null, 2));
}

main();
EOF


echo "Bundling TypeScript code into single JavaScript file..."
npx esbuild vscode-merge-wrapper.ts --bundle --platform=node --format=esm --outfile=vscode-merge.mjs

OUTPUT_FILE="${1:-vscode-merge.mjs}"
OUTPUT_PATH="$START_DIR/$OUTPUT_FILE"

echo "Copying output to: $OUTPUT_PATH"
cp vscode-merge.mjs "$OUTPUT_PATH"

echo ""
echo "✅ Successfully generated: $OUTPUT_PATH"
echo ""
echo "Usage: node $OUTPUT_PATH <base> <input1> <input2>"
echo ""
echo "Test it with:"
echo "  node $OUTPUT_PATH /tmp/base.py /tmp/current.py /tmp/incoming.py"
