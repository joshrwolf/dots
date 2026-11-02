#!/usr/bin/env bash
# Script to extract and bundle VSCode's diff algorithm into a standalone executable

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
git sparse-checkout set src/vs/editor/common/diff src/vs/base/common src/vs/editor/common/core

cd "$WORK_DIR"

echo "Creating wrapper script..."
cat > vscode-diff-wrapper.ts << 'EOF'
#!/usr/bin/env node
import { readFileSync } from 'fs';
import { DefaultLinesDiffComputer } from './vscode/src/vs/editor/common/diff/defaultLinesDiffComputer/defaultLinesDiffComputer.js';

function main() {
    const args = process.argv.slice(2);
    
    // Parse flags
    let showTiming = false;
    let timeoutMs = 5000; // Default timeout: 5 seconds
    let argIdx = 0;
    
    while (argIdx < args.length && args[argIdx].startsWith('-')) {
        if (args[argIdx] === '-b') {
            showTiming = true;
            argIdx++;
        } else if (args[argIdx] === '-T' || args[argIdx] === '--timeout') {
            if (argIdx + 1 >= args.length) {
                console.error(`Error: ${args[argIdx]} requires a value`);
                console.error('Usage: node vscode-diff.mjs [-b] [-T <ms>] <file1> <file2>');
                process.exit(1);
            }
            timeoutMs = parseInt(args[argIdx + 1], 10);
            if (isNaN(timeoutMs) || timeoutMs < 0) {
                console.error('Error: Timeout must be a non-negative number');
                process.exit(1);
            }
            argIdx += 2;
        } else {
            console.error(`Error: Unknown option: ${args[argIdx]}`);
            console.error('Usage: node vscode-diff.mjs [-b] [-T <ms>] <file1> <file2>');
            process.exit(1);
        }
    }
    
    const fileArgs = args.slice(argIdx);
    
    if (fileArgs.length < 2) {
        console.error('Usage: node vscode-diff.mjs [-b] [-T <ms>] <file1> <file2>');
        console.error('Options:');
        console.error('  -b              Show benchmark timing information');
        console.error('  -T <ms>         Set timeout in milliseconds (default: 5000, 0 = no timeout)');
        console.error('  --timeout <ms>  Same as -T');
        process.exit(1);
    }

    const file1Path = fileArgs[0];
    const file2Path = fileArgs[1];

    const file1Content = readFileSync(file1Path, 'utf-8');
    const file2Content = readFileSync(file2Path, 'utf-8');

    // Split by newline - this matches VSCode's behavior where a file ending
    // with \n will have a trailing empty string in the array
    const file1Lines = file1Content.split('\n');
    const file2Lines = file2Content.split('\n');

    // Print header
    console.log('=================================================================');
    console.log('Diff Tool - Computing differences');
    console.log('=================================================================');
    console.log(`Original: ${file1Path} (${file1Lines.length} lines)`);
    console.log(`Modified: ${file2Path} (${file2Lines.length} lines)`);
    console.log('=================================================================\n');

    const diffComputer = new DefaultLinesDiffComputer();
    
    const startTime = performance.now();
    const result = diffComputer.computeDiff(file1Lines, file2Lines, {
        ignoreTrimWhitespace: false,
        maxComputationTimeMs: timeoutMs,
        computeMoves: true,
        extendToSubwords: false,
    });
    const endTime = performance.now();
    const elapsedMs = endTime - startTime;

    // Print results
    console.log('Diff Results:');
    console.log('=================================================================');
    console.log(`Number of changes: ${result.changes.length}`);
    console.log(`Hit timeout: ${result.hitTimeout ? 'yes' : 'no'}`);
    console.log('');
    
    if (!result.changes || result.changes.length === 0) {
        console.log('No differences found - files are identical.');
    } else {
        console.log(`  Changes: ${result.changes.length} line mapping(s)`);
        
        for (let i = 0; i < result.changes.length; i++) {
            const change = result.changes[i];
            const originalRange = change.original;
            const modifiedRange = change.modified;
            
            if (!originalRange || !modifiedRange) {
                console.error('Invalid change object:', change);
                continue;
            }
            
            const innerChanges = change.innerChanges || [];
            const innerCount = innerChanges.length;
            
            // Print line range mapping with inclusive end
            console.log(`    [${i}] Lines ${originalRange.startLineNumber}-${originalRange.endLineNumberExclusive - 1} -> Lines ${modifiedRange.startLineNumber}-${modifiedRange.endLineNumberExclusive - 1}${innerCount > 0 ? ` (${innerCount} inner change${innerCount === 1 ? '' : 's'})` : ' (no inner changes)'}`);
            
            // Print inner changes (character-level)
            for (const inner of innerChanges) {
                const orig = inner.originalRange;
                const mod = inner.modifiedRange;
                console.log(`         Inner: L${orig.startLineNumber}:C${orig.startColumn}-L${orig.endLineNumber}:C${orig.endColumn} -> L${mod.startLineNumber}:C${mod.startColumn}-L${mod.endLineNumber}:C${mod.endColumn}`);
            }
        }
    }

    // Print moves
    console.log('');
    const moves = result.moves || [];
    if (moves.length > 0) {
        console.log(`  Moves: ${moves.length} move(s)`);
        for (let i = 0; i < moves.length; i++) {
            const m = moves[i];
            console.log(`    [${i}] Lines ${m.lineRangeMapping.original.startLineNumber}-${m.lineRangeMapping.original.endLineNumberExclusive - 1} -> Lines ${m.lineRangeMapping.modified.startLineNumber}-${m.lineRangeMapping.modified.endLineNumberExclusive - 1}`);
        }
    } else {
        console.log('  Moves: 0 move(s)');
    }

    console.log('\n=================================================================');
    
    if (showTiming) {
        console.log(`Wall-clock time: ${elapsedMs.toFixed(3)} ms (actual time elapsed)`);
    }
}

main();
EOF


echo "Bundling TypeScript code into single JavaScript file..."
npx esbuild vscode-diff-wrapper.ts --bundle --platform=node --format=esm --outfile=vscode-diff.mjs

OUTPUT_FILE="${1:-vscode-diff.mjs}"
OUTPUT_PATH="$START_DIR/$OUTPUT_FILE"

echo "Copying output to: $OUTPUT_PATH"
cp vscode-diff.mjs "$OUTPUT_PATH"

echo ""
echo "✅ Successfully generated: $OUTPUT_PATH"
echo ""
echo "Usage: node $OUTPUT_PATH <file1> <file2>"
echo ""
echo "Test it with:"
echo "  echo 'line1' > test1.txt"
echo "  echo 'line2' > test2.txt"
echo "  node $OUTPUT_PATH test1.txt test2.txt"
