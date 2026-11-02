/**
 * Memory Leak Test for libvscode-diff
 * 
 * Comprehensive test using Valgrind to detect memory leaks.
 * Tests all major code paths and edge cases.
 */

#include "default_lines_diff_computer.h"
#include "types.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// MSVC compatibility: Use _strdup on Windows
#ifdef _MSC_VER
#define strdup _strdup
#endif

#define TEST_ITERATIONS 100
#define LARGE_FILE_SIZE 1000

static int test_count = 0;
static int test_passed = 0;

#define TEST(name) \
    printf("\n[TEST %d] %s\n", ++test_count, name); \
    printf("----------------------------------------------------------------\n");

#define ASSERT(condition, message) \
    if (!(condition)) { \
        printf("  ✗ FAILED: %s\n", message); \
        return 1; \
    } else { \
        printf("  ✓ %s\n", message); \
        test_passed++; \
    }

/**
 * Test 1: Basic diff computation and cleanup
 */
static int test_basic_diff(void) {
    TEST("Basic diff computation and memory cleanup");
    
    // Use heap-allocated strings to avoid read-only memory issues
    char* original[3];
    char* modified[4];
    
    original[0] = strdup("line 1");
    original[1] = strdup("line 2");
    original[2] = strdup("line 3");
    
    modified[0] = strdup("line 1");
    modified[1] = strdup("modified");
    modified[2] = strdup("line 3");
    modified[3] = strdup("line 4");
    
    DiffOptions options = {
        .ignore_trim_whitespace = false,
        .max_computation_time_ms = 5000,
        .compute_moves = false,
        .extend_to_subwords = false
    };
    
    LinesDiff* diff = compute_diff((const char**)original, 3, (const char**)modified, 4, &options);
    ASSERT(diff != NULL, "Diff computed successfully");
    
    free_lines_diff(diff);
    ASSERT(1, "Memory freed without crash");
    
    // Clean up test data
    for (int i = 0; i < 3; i++) free(original[i]);
    for (int i = 0; i < 4; i++) free(modified[i]);
    
    return 0;
}

/**
 * Test 2: Empty files
 */
static int test_empty_files(void) {
    TEST("Empty file handling");
    
    char* empty[1];
    char* non_empty[2];
    
    empty[0] = strdup("");
    non_empty[0] = strdup("line 1");
    non_empty[1] = strdup("line 2");
    
    DiffOptions options = {0};
    
    LinesDiff* diff1 = compute_diff((const char**)empty, 1, (const char**)non_empty, 2, &options);
    ASSERT(diff1 != NULL, "Empty to non-empty diff");
    free_lines_diff(diff1);
    
    LinesDiff* diff2 = compute_diff((const char**)non_empty, 2, (const char**)empty, 1, &options);
    ASSERT(diff2 != NULL, "Non-empty to empty diff");
    free_lines_diff(diff2);
    
    LinesDiff* diff3 = compute_diff((const char**)empty, 1, (const char**)empty, 1, &options);
    ASSERT(diff3 != NULL, "Empty to empty diff");
    free_lines_diff(diff3);
    
    // Clean up
    free(empty[0]);
    free(non_empty[0]);
    free(non_empty[1]);
    
    return 0;
}

/**
 * Test 3: Identical files
 */
static int test_identical_files(void) {
    TEST("Identical file handling");
    
    char* lines[5];
    for (int i = 0; i < 5; i++) {
        char buf[20];
        snprintf(buf, 20, "line %d", i + 1);
        lines[i] = strdup(buf);
    }
    
    DiffOptions options = {0};
    
    LinesDiff* diff = compute_diff((const char**)lines, 5, (const char**)lines, 5, &options);
    ASSERT(diff != NULL, "Identical files diff");
    ASSERT(diff->changes.count == 0, "No changes detected");
    
    free_lines_diff(diff);
    
    // Clean up
    for (int i = 0; i < 5; i++) free(lines[i]);
    
    return 0;
}

/**
 * Test 4: Large file diff
 */
static int test_large_file(void) {
    TEST("Large file diff (1000 lines)");
    
    // Allocate large arrays
    char** original = malloc(LARGE_FILE_SIZE * sizeof(char*));
    char** modified = malloc(LARGE_FILE_SIZE * sizeof(char*));
    
    ASSERT(original != NULL && modified != NULL, "Memory allocated for test data");
    
    // Generate test data
    for (int i = 0; i < LARGE_FILE_SIZE; i++) {
        char* orig_line = malloc(50);
        char* mod_line = malloc(50);
        
        snprintf(orig_line, 50, "Line %d: original content", i);
        
        if (i % 10 == 0) {
            snprintf(mod_line, 50, "Line %d: MODIFIED content", i);
        } else {
            snprintf(mod_line, 50, "Line %d: original content", i);
        }
        
        original[i] = orig_line;
        modified[i] = mod_line;
    }
    
    DiffOptions options = {0};
    
    LinesDiff* diff = compute_diff((const char**)original, LARGE_FILE_SIZE, (const char**)modified, LARGE_FILE_SIZE, &options);
    ASSERT(diff != NULL, "Large file diff computed");
    
    free_lines_diff(diff);
    
    // Cleanup test data
    for (int i = 0; i < LARGE_FILE_SIZE; i++) {
        free(original[i]);
        free(modified[i]);
    }
    free(original);
    free(modified);
    
    ASSERT(1, "Test data cleaned up");
    
    return 0;
}

/**
 * Test 5: Repeated diff computations (stress test)
 */
static int test_repeated_diffs(void) {
    TEST("Repeated diff computations (100 iterations)");
    
    char* original[5];
    char* modified[5];
    
    original[0] = strdup("line 1");
    original[1] = strdup("line 2");
    original[2] = strdup("line 3");
    original[3] = strdup("line 4");
    original[4] = strdup("line 5");
    
    modified[0] = strdup("line 1");
    modified[1] = strdup("MODIFIED line 2");
    modified[2] = strdup("line 3");
    modified[3] = strdup("NEW line");
    modified[4] = strdup("line 5");
    
    DiffOptions options = {0};
    
    for (int i = 0; i < TEST_ITERATIONS; i++) {
        LinesDiff* diff = compute_diff((const char**)original, 5, (const char**)modified, 5, &options);
        if (diff == NULL) {
            printf("  ✗ FAILED at iteration %d\n", i);
            return 1;
        }
        free_lines_diff(diff);
    }
    
    ASSERT(1, "100 iterations completed without memory issues");
    
    // Clean up
    for (int i = 0; i < 5; i++) {
        free(original[i]);
        free(modified[i]);
    }
    
    return 0;
}

/**
 * Test 6: Different options combinations
 */
static int test_options_combinations(void) {
    TEST("Different option combinations");
    
    char* original[3];
    char* modified[3];
    
    original[0] = strdup("  line 1  ");
    original[1] = strdup("line 2");
    original[2] = strdup("line 3");
    
    modified[0] = strdup("line 1");
    modified[1] = strdup("  line 2  ");
    modified[2] = strdup("line 3");
    
    DiffOptions options1 = {
        .ignore_trim_whitespace = false,
        .max_computation_time_ms = 5000,
        .compute_moves = false,
        .extend_to_subwords = false
    };
    
    DiffOptions options2 = {
        .ignore_trim_whitespace = true,
        .max_computation_time_ms = 5000,
        .compute_moves = false,
        .extend_to_subwords = true
    };
    
    LinesDiff* diff1 = compute_diff((const char**)original, 3, (const char**)modified, 3, &options1);
    ASSERT(diff1 != NULL, "Diff with ignore_trim_whitespace=false");
    free_lines_diff(diff1);
    
    LinesDiff* diff2 = compute_diff((const char**)original, 3, (const char**)modified, 3, &options2);
    ASSERT(diff2 != NULL, "Diff with ignore_trim_whitespace=true");
    free_lines_diff(diff2);
    
    // Clean up
    for (int i = 0; i < 3; i++) {
        free(original[i]);
        free(modified[i]);
    }
    
    return 0;
}

/**
 * Test 7: Character-level changes
 */
static int test_char_level_changes(void) {
    TEST("Character-level change detection");
    
    char* original[2];
    char* modified[2];
    
    original[0] = strdup("The quick brown fox");
    original[1] = strdup("jumps over the lazy dog");
    
    modified[0] = strdup("The quick red fox");
    modified[1] = strdup("jumps over the lazy cat");
    
    DiffOptions options = {
        .ignore_trim_whitespace = false,
        .max_computation_time_ms = 5000,
        .compute_moves = false,
        .extend_to_subwords = false
    };
    
    LinesDiff* diff = compute_diff((const char**)original, 2, (const char**)modified, 2, &options);
    ASSERT(diff != NULL, "Character-level diff computed");
    
    // Check that inner changes are detected
    ASSERT(diff->changes.count > 0, "Changes detected");
    if (diff->changes.count > 0) {
        ASSERT(diff->changes.mappings[0].inner_changes != NULL, 
               "Character-level changes detected");
    }
    
    free_lines_diff(diff);
    
    // Clean up
    for (int i = 0; i < 2; i++) {
        free(original[i]);
        free(modified[i]);
    }
    
    return 0;
}

/**
 * Test 8: NULL pointer safety
 */
static int test_null_safety(void) {
    TEST("NULL pointer safety");
    
    // Test free_lines_diff with NULL
    free_lines_diff(NULL);
    ASSERT(1, "free_lines_diff(NULL) handled safely");
    
    return 0;
}

/**
 * Main test runner
 */
int main(void) {
    printf("╔════════════════════════════════════════════════════════════╗\n");
    printf("║          MEMORY LEAK TEST (Valgrind)                       ║\n");
    printf("║          libvscode-diff Comprehensive Testing              ║\n");
    printf("╚════════════════════════════════════════════════════════════╝\n");
    
    int failed = 0;
    
    failed += test_basic_diff();
    failed += test_empty_files();
    failed += test_identical_files();
    failed += test_large_file();
    failed += test_repeated_diffs();
    failed += test_options_combinations();
    failed += test_char_level_changes();
    failed += test_null_safety();
    
    printf("\n");
    printf("════════════════════════════════════════════════════════════\n");
    printf("Test Summary\n");
    printf("════════════════════════════════════════════════════════════\n");
    printf("Total tests run:    %d\n", test_count);
    printf("Assertions passed:  %d\n", test_passed);
    printf("Tests failed:       %d\n", failed);
    printf("\n");
    
    if (failed == 0) {
        printf("✓ ALL TESTS PASSED\n");
        printf("\nRun with Valgrind to check for memory leaks:\n");
        printf("  valgrind --leak-check=full --show-leak-kinds=all \\\n");
        printf("           --error-exitcode=1 ./test_memory_leak\n");
        printf("\n");
        return 0;
    } else {
        printf("✗ SOME TESTS FAILED\n\n");
        return 1;
    }
}
