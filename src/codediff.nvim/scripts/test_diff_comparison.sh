#!/bin/bash

# Test script to compare C diff tool and Node vscode-diff.mjs outputs
# Dynamically tests top N most revised files from git history (origin/main)
#
# Usage: ./test_diff_comparison.sh [OPTIONS] [REPO_PATH]
#   -q, --quiet      Quiet mode: only show summary (tests/mismatches)
#   (no options)     Normal mode: show progress and summary
#   -v, --verbose    Verbose mode: show detailed output
#   REPO_PATH        Optional path to git repository to test (default: current repo)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOL_REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
TARGET_REPO_ROOT=""
C_DIFF="$TOOL_REPO_ROOT/build/libvscode-diff/diff"
NODE_DIFF="$TOOL_REPO_ROOT/vscode-diff.mjs"
TEMP_DIR="/tmp/diff_comparison_$$"
EXAMPLE_DIR=""

# Configuration: Number of top revised files to test
NUM_TOP_FILES=10
TESTS_PER_FILE=30
# Use origin/main as the reference point for consistent results
BASE_REF="origin/main"
# Verbosity level: 0=quiet, 1=normal, 2=verbose
VERBOSITY=1
# Sort mode: frequency (default) or size
SORT_MODE="frequency"
# OpenMP: enabled by default
ENABLE_OPENMP=ON

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -q|--quiet)
            VERBOSITY=0
            shift
            ;;
        -v|--verbose)
            VERBOSITY=2
            shift
            ;;
        -s|--size)
            SORT_MODE="size"
            shift
            ;;
        --no-openmp)
            ENABLE_OPENMP=OFF
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [OPTIONS] [REPO_PATH]"
            echo ""
            echo "Options:"
            echo "  -q, --quiet      Quiet mode: only show summary (tests/mismatches)"
            echo "                   Perfect for comparing test runs"
            echo "  (no options)     Normal mode: show progress and summary"
            echo "  -v, --verbose    Verbose mode: show all details and performance"
            echo "  -s, --size       Sort files by size (default: sort by revision frequency)"
            echo "  --no-openmp      Disable OpenMP (build with sequential diff)"
            echo "  -h, --help       Show this help message"
            echo ""
            echo "Arguments:"
            echo "  REPO_PATH        Path to git repository to test (default: current repo)"
            exit 0
            ;;
        -*)
            echo "Unknown option: $1"
            echo "Use -h or --help for usage information"
            exit 1
            ;;
        *)
            TARGET_REPO_ROOT="$1"
            shift
            ;;
    esac
done

# Set default repo path if not provided
if [ -z "$TARGET_REPO_ROOT" ]; then
    TARGET_REPO_ROOT="$TOOL_REPO_ROOT"
else
    # Resolve to absolute path
    TARGET_REPO_ROOT="$(cd "$TARGET_REPO_ROOT" && pwd)"
    if [ ! -d "$TARGET_REPO_ROOT/.git" ]; then
        echo "Error: $TARGET_REPO_ROOT is not a git repository" >&2
        exit 1
    fi
fi

# Auto-detect default branch (try origin/main, origin/master, HEAD)
if git -C "$TARGET_REPO_ROOT" rev-parse --verify origin/main >/dev/null 2>&1; then
    BASE_REF="origin/main"
elif git -C "$TARGET_REPO_ROOT" rev-parse --verify origin/master >/dev/null 2>&1; then
    BASE_REF="origin/master"
else
    BASE_REF="HEAD"
fi

# Set example directory in /tmp with repo-specific name
REPO_NAME=$(basename "$TARGET_REPO_ROOT")
EXAMPLE_DIR="/tmp/diff_comparison_examples_${REPO_NAME}_$$"

mkdir -p "$TEMP_DIR"

# Always rebuild C diff binary to ensure latest changes
if [ $VERBOSITY -ge 1 ]; then
    if [ "$ENABLE_OPENMP" = "OFF" ]; then
        echo "Building C diff binary with clean build (OpenMP disabled)..."
    else
        echo "Building C diff binary with clean build..."
    fi
fi
cd "$TOOL_REPO_ROOT"
# Clean build directory to ensure OpenMP setting takes effect
make clean > /dev/null 2>&1
# Configure and build
cmake -B build -DENABLE_OPENMP=$ENABLE_OPENMP > /dev/null 2>&1
cmake --build build --target diff > /dev/null 2>&1
if [ ! -f "$C_DIFF" ]; then
    echo "Error: Failed to build C diff binary" >&2
    exit 1
fi
if [ $VERBOSITY -ge 1 ]; then
    echo "✓ C diff binary built successfully"
    echo ""
fi

if [ ! -f "$NODE_DIFF" ]; then
    if [ $VERBOSITY -ge 1 ]; then
        echo "Node diff binary not found. Building..."
    fi
    "$SCRIPT_DIR/build-vscode-diff.sh" > /dev/null 2>&1
    if [ ! -f "$NODE_DIFF" ]; then
        echo "Error: Failed to build Node diff binary" >&2
        exit 1
    fi
fi

# Function to generate example files for given file list
generate_example_files() {
    shift  # Remove first argument (num_files - kept for backward compatibility)
    local files=("$@")
    
    if [ $VERBOSITY -ge 2 ]; then
        echo "Generating example files for ${#files[@]} selected files from $BASE_REF..."
    fi
    
    # Create example directory
    mkdir -p "$EXAMPLE_DIR"

    # Calculate minimum versions needed for TESTS_PER_FILE tests
    # We diff the most recent version against progressively older ones
    # So we need TESTS_PER_FILE + 1 versions (1 recent + TESTS_PER_FILE older)
    local MAX_VERSIONS=$((TESTS_PER_FILE + 1))
    
    # For each top file, save a limited number of its git history versions up to BASE_REF
    for file in "${files[@]}"; do
        if [ $VERBOSITY -ge 2 ]; then
            echo "Processing $file..."
        fi
        
        # Check if file exists at BASE_REF
        if ! git -C "$TARGET_REPO_ROOT" cat-file -e "$BASE_REF:$file" 2>/dev/null; then
            if [ $VERBOSITY -ge 2 ]; then
                echo "  Warning: $file not found at $BASE_REF, skipping"
            fi
            continue
        fi
        
        local basename=$(basename "$file")
        
        # Get commits that modified this file - limit to MAX_VERSIONS most recent
        # This is much faster than getting all commits for highly-revised files
        local commits_reverse=($(git -C "$TARGET_REPO_ROOT" log "$BASE_REF" -n $MAX_VERSIONS --format=%H -- "$file"))
        
        # Reverse the array to get chronological order
        local commits=()
        for ((i=${#commits_reverse[@]}-1; i>=0; i--)); do
            commits+=("${commits_reverse[i]}")
        done
        
        if [ $VERBOSITY -ge 2 ]; then
            echo "  Found ${#commits[@]} commits (limited to $MAX_VERSIONS)"
        fi
        
        # Save each version with sequence number and commit hash for ordering
        local count=0
        for idx in "${!commits[@]}"; do
            local commit="${commits[$idx]}"
            # Use zero-padded index for proper sorting (e.g., 001, 002, ...)
            local seq=$(printf "%03d" $idx)
            local output_file="$EXAMPLE_DIR/${basename}_${seq}_${commit}"
            
            # Extract file content - skip if extraction fails (don't do expensive fallback)
            if git -C "$TARGET_REPO_ROOT" show "$commit:$file" > "$output_file" 2>/dev/null; then
                count=$((count + 1))
            else
                rm -f "$output_file"
            fi
        done
        
        if [ $VERBOSITY -ge 2 ]; then
            echo "  Saved $count versions in chronological order"
        fi
    done
    
    if [ $VERBOSITY -ge 2 ]; then
        echo ""
        echo "Done! Example files generated in $EXAMPLE_DIR"
        echo "Total files: $(ls -1 "$EXAMPLE_DIR" | wc -l)"
    fi
}

# Get top N files from git history up to BASE_REF
if [ $VERBOSITY -ge 1 ]; then
    echo "Testing repository: $TARGET_REPO_ROOT"
    if [ "$SORT_MODE" = "size" ]; then
        echo "Finding top $NUM_TOP_FILES largest files (up to $BASE_REF)..."
    else
        echo "Finding top $NUM_TOP_FILES most revised files from git history (up to $BASE_REF)..."
    fi
fi

# Pre-compute revision counts in a single git log pass (much faster than per-file queries)
REVISION_COUNTS_FILE="$TEMP_DIR/revision_counts.txt"
git -C "$TARGET_REPO_ROOT" log "$BASE_REF" --name-only --format="" | sort | uniq -c | awk '$1 >= 5 {print $1, $2}' > "$REVISION_COUNTS_FILE"

if [ "$SORT_MODE" = "size" ]; then
    # Sort by file size - get files at BASE_REF with size, join with revision counts
    SIZE_FILE="$TEMP_DIR/file_sizes.txt"
    git -C "$TARGET_REPO_ROOT" ls-tree -r -l "$BASE_REF" | awk '{size=$4; path=""; for(i=5;i<=NF;i++) path=path (i>5?" ":"") $i; print path "\t" size}' | sort > "$SIZE_FILE"
    
    # Join size info with revision counts (only files with 5+ revisions), sort by size
    TOP_FILES=($(awk '{print $2}' "$REVISION_COUNTS_FILE" | sort | join -t$'\t' - "$SIZE_FILE" | sort -t$'\t' -k2 -rn | head -$NUM_TOP_FILES | cut -f1))
else
    # Sort by revision frequency (default) - filter to files that exist at BASE_REF
    EXISTING_FILES_FILE="$TEMP_DIR/existing_files.txt"
    git -C "$TARGET_REPO_ROOT" ls-tree -r --name-only "$BASE_REF" | sort > "$EXISTING_FILES_FILE"
    
    # Join with existing files to filter, already sorted by revision count
    TOP_FILES=($(awk '{print $2, $1}' "$REVISION_COUNTS_FILE" | sort | join - "$EXISTING_FILES_FILE" | sort -k2 -rn | head -$NUM_TOP_FILES | awk '{print $1}'))
fi

# Check if we need to regenerate example files
NEED_REGENERATE=false
for TOP_FILE in "${TOP_FILES[@]}"; do
    BASENAME=$(basename "$TOP_FILE")
    FILES_COUNT=$(ls -1 "$EXAMPLE_DIR"/${BASENAME}_* 2>/dev/null | wc -l)
    if [ $FILES_COUNT -eq 0 ]; then
        NEED_REGENERATE=true
        break
    fi
done

if [ "$NEED_REGENERATE" = true ]; then
    if [ $VERBOSITY -ge 1 ]; then
        echo "Example files missing or incomplete. Regenerating..."
    fi
    generate_example_files $NUM_TOP_FILES "${TOP_FILES[@]}"
    if [ $VERBOSITY -ge 1 ]; then
        echo ""
    fi
fi

if [ $VERBOSITY -ge 2 ]; then
    if [ "$SORT_MODE" = "size" ]; then
        echo "Top files by size (as of $BASE_REF):"
        for i in "${!TOP_FILES[@]}"; do
            # Get file size in bytes
            SIZE_BYTES=$(git -C "$TARGET_REPO_ROOT" cat-file -s "$BASE_REF:${TOP_FILES[$i]}" 2>/dev/null || echo "0")
            SIZE_KB=$(awk "BEGIN {printf \"%.1f\", $SIZE_BYTES/1024}")
            LINES=$(git -C "$TARGET_REPO_ROOT" show "$BASE_REF:${TOP_FILES[$i]}" 2>/dev/null | wc -l || echo "0")
            echo "  $((i+1)). ${TOP_FILES[$i]} (${SIZE_KB}KB, ${LINES} lines)"
        done
    else
        echo "Top revised files (as of $BASE_REF, with rename tracking):"
        for i in "${!TOP_FILES[@]}"; do
            # Use --follow to track renames for each individual file
            REVISIONS=$(git -C "$TARGET_REPO_ROOT" log "$BASE_REF" --follow --oneline -- "${TOP_FILES[$i]}" | wc -l)
            echo "  $((i+1)). ${TOP_FILES[$i]} ($REVISIONS revisions)"
        done
    fi
    echo ""
fi

# Collect version files for each top file
declare -a FILE_GROUPS
declare -A FILE_METRICS  # Store file metrics (lines, size)
for TOP_FILE in "${TOP_FILES[@]}"; do
    BASENAME=$(basename "$TOP_FILE")
    # Files are now named: basename_SEQ_HASH, sort by SEQ for chronological order
    FILES=($(ls -1 "$EXAMPLE_DIR"/${BASENAME}_* 2>/dev/null | sort))
    if [ ${#FILES[@]} -gt 0 ]; then
        FILE_GROUPS+=("${#FILES[@]}")
        eval "FILES_${BASENAME//[^a-zA-Z0-9]/_}=(${FILES[@]})"
        
        # Get metrics from the latest version (last file in chronological order)
        LATEST_FILE="${FILES[-1]}"
        LINES=$(wc -l < "$LATEST_FILE" 2>/dev/null || echo "0")
        SIZE_BYTES=$(stat -f%z "$LATEST_FILE" 2>/dev/null || stat -c%s "$LATEST_FILE" 2>/dev/null || echo "0")
        SIZE_KB=$(awk "BEGIN {printf \"%.1f\", $SIZE_BYTES/1024}")
        FILE_METRICS["$BASENAME"]="${LINES}L ${SIZE_KB}KB"
        
        if [ $VERBOSITY -ge 2 ]; then
            echo "Found ${#FILES[@]} versions of $BASENAME (chronologically ordered)"
        fi
    fi
done
if [ $VERBOSITY -ge 2 ]; then
    echo ""
fi

TOTAL_TESTS=0
MISMATCHES=0
MISMATCH_DETAILS=""
FIRST_MISMATCH_C=""
FIRST_MISMATCH_NODE=""

# Timing arrays per file
declare -A C_TIMES
declare -A NODE_TIMES
declare -A TEST_COUNTS

# Function to test a pair of files
test_pair() {
    local FILE1="$1"
    local FILE2="$2"
    local TEST_ID="$3"
    local FILE_GROUP="$4"
    
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    
    # Get base filenames for display
    BASE1=$(basename "$FILE1")
    BASE2=$(basename "$FILE2")
    
    # Run C diff tool with timing (no timeout for accurate comparison)
    C_OUTPUT="$TEMP_DIR/c_output_${TEST_ID}.txt"
    C_START=$(($(date +%s%N)/1000000))
    "$C_DIFF" -T 0 "$FILE1" "$FILE2" > "$C_OUTPUT" 2>&1
    C_EXIT=$?
    C_END=$(($(date +%s%N)/1000000))
    C_TIME=$((C_END - C_START))
    # Sanity check: ensure non-negative timing
    [ $C_TIME -lt 0 ] && C_TIME=0
    
    # Run Node diff tool with timing (no timeout for accurate comparison)
    NODE_OUTPUT="$TEMP_DIR/node_output_${TEST_ID}.txt"
    NODE_START=$(($(date +%s%N)/1000000))
    node "$NODE_DIFF" -T 0 "$FILE1" "$FILE2" > "$NODE_OUTPUT" 2>&1
    NODE_EXIT=$?
    NODE_END=$(($(date +%s%N)/1000000))
    NODE_TIME=$((NODE_END - NODE_START))
    # Sanity check: ensure non-negative timing
    [ $NODE_TIME -lt 0 ] && NODE_TIME=0
    
    # Accumulate timing stats
    C_TIMES[$FILE_GROUP]=$((${C_TIMES[$FILE_GROUP]:-0} + C_TIME))
    NODE_TIMES[$FILE_GROUP]=$((${NODE_TIMES[$FILE_GROUP]:-0} + NODE_TIME))
    TEST_COUNTS[$FILE_GROUP]=$((${TEST_COUNTS[$FILE_GROUP]:-0} + 1))
    
    # Compare outputs (normalize trailing newlines to avoid false positives)
    if ! diff -q "$C_OUTPUT" "$NODE_OUTPUT" > /dev/null 2>&1; then
        MISMATCHES=$((MISMATCHES + 1))
        if [ -z "$FIRST_MISMATCH_C" ]; then
            FIRST_MISMATCH_C="$C_OUTPUT"
            FIRST_MISMATCH_NODE="$NODE_OUTPUT"
        fi
        MISMATCH_DETAILS="${MISMATCH_DETAILS}Mismatch #${MISMATCHES} (Test #${TOTAL_TESTS}):\n"
        MISMATCH_DETAILS="${MISMATCH_DETAILS}  Files: $BASE1 vs $BASE2\n"
        MISMATCH_DETAILS="${MISMATCH_DETAILS}  C exit: $C_EXIT, Node exit: $NODE_EXIT\n"
        MISMATCH_DETAILS="${MISMATCH_DETAILS}  C output: $C_OUTPUT\n"
        MISMATCH_DETAILS="${MISMATCH_DETAILS}  Node output: $NODE_OUTPUT\n\n"
    fi
    
    # Progress indicator every 10 tests (normal and verbose only)
    if [ $VERBOSITY -ge 1 ] && [ $((TOTAL_TESTS % 10)) -eq 0 ]; then
        echo "Progress: $TOTAL_TESTS tests completed, $MISMATCHES mismatches found"
    fi
}

# Test each file group with real-world commit distances
for FILE_IDX in "${!TOP_FILES[@]}"; do
    TOP_FILE="${TOP_FILES[$FILE_IDX]}"
    BASENAME=$(basename "$TOP_FILE")
    VAR_NAME="FILES_${BASENAME//[^a-zA-Z0-9]/_}[@]"
    eval "FILE_ARRAY=(\${$VAR_NAME})"
    NUM_FILES=${#FILE_ARRAY[@]}
    
    if [ $VERBOSITY -ge 1 ]; then
        echo "Testing $BASENAME versions (target: $TESTS_PER_FILE tests)..."
        if [ $VERBOSITY -ge 2 ]; then
            echo "  Strategy: diff most recent against progressively older versions"
        fi
    fi
    TESTS_BEFORE=$TOTAL_TESTS
    
    # Diff most recent version against progressively older versions
    # This simulates real-world usage: comparing current code with history
    # FILE_ARRAY is in chronological order, so last element is most recent
    LATEST_IDX=$((NUM_FILES - 1))
    for ((distance=1; distance<$NUM_FILES && (TOTAL_TESTS - TESTS_BEFORE)<$TESTS_PER_FILE; distance++)); do
        OLD_IDX=$((LATEST_IDX - distance))
        if [ $OLD_IDX -ge 0 ]; then
            test_pair "${FILE_ARRAY[$OLD_IDX]}" "${FILE_ARRAY[$LATEST_IDX]}" "${BASENAME//[^a-zA-Z0-9]/_}_d${distance}" "$BASENAME"
        fi
    done
    if [ $VERBOSITY -ge 1 ]; then
        echo ""
    fi
done

# Test crafted move detection pairs
PAIRS_DIR="$TOOL_REPO_ROOT/scripts/test_pairs"
if [ -d "$PAIRS_DIR" ]; then
    if [ $VERBOSITY -ge 1 ]; then
        echo ""
        echo "Testing crafted move detection pairs..."
    fi
    for PAIR_DIR in "$PAIRS_DIR"/*/; do
        if [ -f "$PAIR_DIR/original.txt" ] && [ -f "$PAIR_DIR/modified.txt" ]; then
            PAIR_NAME=$(basename "$PAIR_DIR")
            test_pair "$PAIR_DIR/original.txt" "$PAIR_DIR/modified.txt" "pair_${PAIR_NAME}" "crafted"
            if [ $VERBOSITY -ge 1 ]; then
                echo "  Tested: $PAIR_NAME"
            fi
        fi
    done
fi

# Output based on verbosity level
if [ $VERBOSITY -eq 0 ]; then
    # Quiet mode: single line for easy comparison
    echo "$TOTAL_TESTS $MISMATCHES"
else
    # Normal and verbose modes
    echo ""
    echo "========================================"
    echo "SUMMARY"
    echo "========================================"
    echo "Total tests run: $TOTAL_TESTS"
    echo "Mismatches found: $MISMATCHES"
    echo ""
    
    if [ $MISMATCHES -gt 0 ]; then
        if [ $VERBOSITY -ge 2 ]; then
            echo "MISMATCH DETAILS:"
            echo "========================================"
            echo -e "$MISMATCH_DETAILS"
            echo ""
            echo "Showing first mismatch in detail:"
            echo "========================================"
            
            if [ -f "$FIRST_MISMATCH_C" ] && [ -f "$FIRST_MISMATCH_NODE" ]; then
                echo "C diff output:"
                echo "---"
                head -50 "$FIRST_MISMATCH_C"
                echo ""
                echo "Node diff output:"
                echo "---"
                head -50 "$FIRST_MISMATCH_NODE"
                echo ""
                echo "Diff between outputs:"
                echo "---"
                diff -u "$FIRST_MISMATCH_C" "$FIRST_MISMATCH_NODE" | head -100
            fi
        else
            echo "⚠ Mismatches detected. Run with -v or --verbose to see details."
        fi
    else
        echo "✓ All tests passed! No mismatches found."
    fi
    
    # Performance comparison (normal and verbose modes)
    echo ""
    echo "========================================"
    echo "PERFORMANCE COMPARISON"
    echo "========================================"
    
    if [ $VERBOSITY -eq 1 ]; then
        # Normal mode: condensed summary
        for FILE_IDX in "${!TOP_FILES[@]}"; do
            TOP_FILE="${TOP_FILES[$FILE_IDX]}"
            BASENAME=$(basename "$TOP_FILE")
            
            if [ ${TEST_COUNTS[$BASENAME]:-0} -gt 0 ]; then
                C_AVG=$(( ${C_TIMES[$BASENAME]} / ${TEST_COUNTS[$BASENAME]} ))
                NODE_AVG=$(( ${NODE_TIMES[$BASENAME]} / ${TEST_COUNTS[$BASENAME]} ))
                
                # Only show if both timings are valid (positive)
                if [ $C_AVG -gt 0 ] && [ $NODE_AVG -gt 0 ]; then
                    RATIO=$(( (NODE_AVG * 100) / C_AVG ))
                    echo "$BASENAME: C=${C_AVG}ms, Node=${NODE_AVG}ms (${RATIO}%)"
                elif [ $C_AVG -gt 0 ] || [ $NODE_AVG -gt 0 ]; then
                    # At least one timing available
                    echo "$BASENAME: C=${C_AVG}ms, Node=${NODE_AVG}ms (timing error)"
                fi
            fi
        done
    else
        # Verbose mode: detailed output
        for FILE_IDX in "${!TOP_FILES[@]}"; do
            TOP_FILE="${TOP_FILES[$FILE_IDX]}"
            BASENAME=$(basename "$TOP_FILE")
            
            if [ ${TEST_COUNTS[$BASENAME]:-0} -gt 0 ]; then
                C_AVG=$(( ${C_TIMES[$BASENAME]} / ${TEST_COUNTS[$BASENAME]} ))
                NODE_AVG=$(( ${NODE_TIMES[$BASENAME]} / ${TEST_COUNTS[$BASENAME]} ))
                
                # Include file metrics in the output
                METRICS="${FILE_METRICS[$BASENAME]}"
                echo "$BASENAME [$METRICS] (${TEST_COUNTS[$BASENAME]} tests):"
                echo "  C diff:    ${C_AVG} ms average"
                echo "  Node diff: ${NODE_AVG} ms average"
                
                # Only show ratio if both timings are valid
                if [ $C_AVG -gt 0 ] && [ $NODE_AVG -gt 0 ]; then
                    RATIO=$(( (NODE_AVG * 100) / C_AVG ))
                    echo "  Node/C ratio: ${RATIO}%"
                elif [ $C_AVG -le 0 ] || [ $NODE_AVG -le 0 ]; then
                    echo "  ⚠ Timing error detected (negative or zero values)"
                fi
                echo ""
            fi
        done
    fi
fi

# Cleanup
rm -rf "$TEMP_DIR"
rm -rf "$EXAMPLE_DIR"

# Always exit 0 - mismatches are reported in output, not exit code
# The regression check compares outputs, not exit codes
exit 0
