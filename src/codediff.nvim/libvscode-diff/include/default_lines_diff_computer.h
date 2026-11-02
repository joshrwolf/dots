#ifndef DEFAULT_LINES_DIFF_COMPUTER_H
#define DEFAULT_LINES_DIFF_COMPUTER_H

#include "types.h"

// DLL export/import declarations for Windows
#ifdef _WIN32
  #ifdef BUILDING_DLL
    #define DLL_EXPORT __declspec(dllexport)
  #else
    #define DLL_EXPORT __declspec(dllimport)
  #endif
#else
  #define DLL_EXPORT
#endif

/**
 * Compute diff between two files.
 * 
 * Main entry point for computing a complete diff with line and character
 * level changes. Implements VSCode's DefaultLinesDiffComputer.computeDiff().
 * 
 * @param original_lines Original file lines
 * @param original_count Number of lines in original
 * @param modified_lines Modified file lines  
 * @param modified_count Number of lines in modified
 * @param options Diff computation options
 * @return LinesDiff structure (caller must free with free_lines_diff())
 * 
 * VSCode Reference: defaultLinesDiffComputer.ts computeDiff()
 * VSCode Parity: 100% (excluding computeMoves and DP algorithm)
 */
DLL_EXPORT LinesDiff *compute_diff(const char **original_lines, int original_count,
                        const char **modified_lines, int modified_count,
                        const DiffOptions *options);

/**
 * Free LinesDiff structure and all contained data.
 * 
 * @param diff LinesDiff to free (can be NULL)
 */
DLL_EXPORT void free_lines_diff(LinesDiff *diff);

/**
 * Get library version string.
 * 
 * @return Version string
 */
DLL_EXPORT const char *get_version(void);

#endif // DEFAULT_LINES_DIFF_COMPUTER_H
