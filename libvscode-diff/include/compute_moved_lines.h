#ifndef COMPUTE_MOVED_LINES_H
#define COMPUTE_MOVED_LINES_H

#include "types.h"
#include <stdbool.h>

/**
 * Compute moved line blocks between two files.
 *
 * Port of VSCode's computeMovedLines() from computeMovedLines.ts.
 *
 * @param changes        Array of DetailedLineRangeMappings (the diff changes)
 * @param change_count   Number of changes
 * @param original_lines Original file lines
 * @param original_count Number of original lines
 * @param modified_lines Modified file lines
 * @param modified_count Number of modified lines
 * @param hashed_original Trimmed-hash of each original line (0-indexed, length = original_count)
 * @param hashed_modified Trimmed-hash of each modified line (0-indexed, length = modified_count)
 * @param timeout_ms     Timeout in milliseconds (0 = infinite)
 * @param out_moves      Output: array of MovedText (caller must free .moves)
 */
void compute_moved_lines(
    const DetailedLineRangeMapping *changes, int change_count,
    const char **original_lines, int original_count,
    const char **modified_lines, int modified_count,
    const uint32_t *hashed_original, const uint32_t *hashed_modified,
    int timeout_ms,
    MovedTextArray *out_moves);

#endif // COMPUTE_MOVED_LINES_H
