// ============================================================================
// Port of VSCode's computeMovedLines.ts to C
// ============================================================================
//
// VSCode Reference:
//   src/vs/editor/common/diff/defaultLinesDiffComputer/computeMovedLines.ts
//   src/vs/editor/common/diff/defaultLinesDiffComputer/utils.ts (LineRangeFragment)
//
// All thresholds and logic match VSCode exactly.
// ============================================================================

#include <stdlib.h>
#include <string.h>

#include "compute_moved_lines.h"
#include "myers.h"
#include "sequence.h"
#include "utils.h"

// ============================================================================
// Timeout helper
// ============================================================================

typedef struct {
  int timeout_ms;
  int64_t start_time_ms;
} MoveTimeout;

static bool timeout_is_valid(const MoveTimeout *t) {
  if (t->timeout_ms <= 0)
    return true;
  return (get_current_time_ms() - t->start_time_ms) < t->timeout_ms;
}

// ============================================================================
// LineRange helpers (1-based, end exclusive — matching VSCode)
// ============================================================================

static int lr_length(LineRange r) { return r.end_line - r.start_line; }
static bool lr_is_empty(LineRange r) { return r.start_line == r.end_line; }

static LineRange lr_new(int start, int end_excl) {
  LineRange r = {start, end_excl};
  return r;
}

static LineRange lr_join(LineRange a, LineRange b) {
  int s = a.start_line < b.start_line ? a.start_line : b.start_line;
  int e = a.end_line > b.end_line ? a.end_line : b.end_line;
  return lr_new(s, e);
}

static LineRange lr_delta(LineRange r, int delta) {
  return lr_new(r.start_line + delta, r.end_line + delta);
}

// ============================================================================
// MovedText (LineRangeMapping in VSCode) dynamic array
// ============================================================================

typedef struct {
  MovedText *items;
  int count;
  int cap;
} MoveArray;

static void ma_init(MoveArray *a) {
  a->items = NULL;
  a->count = 0;
  a->cap = 0;
}

static void ma_push(MoveArray *a, MovedText m) {
  if (a->count >= a->cap) {
    a->cap = a->cap == 0 ? 8 : a->cap * 2;
    a->items = (MovedText *)realloc(a->items, (size_t)a->cap * sizeof(MovedText));
  }
  a->items[a->count++] = m;
}

static void ma_free(MoveArray *a) {
  free(a->items);
  a->items = NULL;
  a->count = 0;
  a->cap = 0;
}

// ============================================================================
// LineRangeFragment — histogram-based text similarity (from utils.ts)
// ============================================================================

// Using a fixed-size histogram indexed by character code.
// VSCode uses a Map<string, number> for character keys, but since we deal
// with individual characters, a direct array indexed by char value works.
// We use 65536 buckets to cover the full BMP (matching JS string indexing).

#define HIST_SIZE 65536

typedef struct {
  LineRange range;
  int source_idx; // index into changes array
  int total_count;
  int *histogram; // dynamically allocated [HIST_SIZE]
} LineRangeFragment;

static LineRangeFragment lrf_create(LineRange range, const char **lines, int source_idx) {
  LineRangeFragment f;
  f.range = range;
  f.source_idx = source_idx;
  f.histogram = (int *)calloc(HIST_SIZE, sizeof(int));
  int counter = 0;
  for (int i = range.start_line - 1; i < range.end_line - 1; i++) {
    const char *line = lines[i];
    for (int j = 0; line[j] != '\0'; j++) {
      counter++;
      unsigned char ch = (unsigned char)line[j];
      f.histogram[ch]++;
    }
    counter++;
    f.histogram[(unsigned char)'\n']++;
  }
  f.total_count = counter;
  return f;
}

static double lrf_compute_similarity(const LineRangeFragment *a, const LineRangeFragment *b) {
  int sum_diff = 0;
  for (int i = 0; i < HIST_SIZE; i++) {
    int va = a->histogram[i];
    int vb = b->histogram[i];
    int d = va - vb;
    sum_diff += d < 0 ? -d : d;
  }
  return 1.0 - (double)sum_diff / (double)(a->total_count + b->total_count);
}

static void lrf_free(LineRangeFragment *f) {
  free(f->histogram);
  f->histogram = NULL;
}

// ============================================================================
// SetMap<string, {range: LineRange}> — multimap for 3-line hash windows
// ============================================================================

typedef struct SetMapEntry {
  char *key;
  LineRange *ranges;
  int count;
  int cap;
  struct SetMapEntry *next;
} SetMapEntry;

#define SETMAP_BUCKETS 4096

typedef struct {
  SetMapEntry *buckets[SETMAP_BUCKETS];
} SetMap;

static void setmap_init(SetMap *m) { memset(m->buckets, 0, sizeof(m->buckets)); }

static unsigned setmap_hash(const char *key) {
  unsigned h = 5381;
  while (*key) {
    h = h * 33 + (unsigned char)*key;
    key++;
  }
  return h % SETMAP_BUCKETS;
}

static SetMapEntry *setmap_get_or_create(SetMap *m, const char *key) {
  unsigned h = setmap_hash(key);
  SetMapEntry *e = m->buckets[h];
  while (e) {
    if (strcmp(e->key, key) == 0)
      return e;
    e = e->next;
  }
  e = (SetMapEntry *)malloc(sizeof(SetMapEntry));
  e->key = (char *)malloc(strlen(key) + 1);
  strcpy(e->key, key);
  e->ranges = NULL;
  e->count = 0;
  e->cap = 0;
  e->next = m->buckets[h];
  m->buckets[h] = e;
  return e;
}

static void setmap_add(SetMap *m, const char *key, LineRange range) {
  SetMapEntry *e = setmap_get_or_create(m, key);
  if (e->count >= e->cap) {
    e->cap = e->cap == 0 ? 4 : e->cap * 2;
    e->ranges = (LineRange *)realloc(e->ranges, (size_t)e->cap * sizeof(LineRange));
  }
  e->ranges[e->count++] = range;
}

static SetMapEntry *setmap_find(SetMap *m, const char *key) {
  unsigned h = setmap_hash(key);
  SetMapEntry *e = m->buckets[h];
  while (e) {
    if (strcmp(e->key, key) == 0)
      return e;
    e = e->next;
  }
  return NULL;
}

static void setmap_free(SetMap *m) {
  for (int i = 0; i < SETMAP_BUCKETS; i++) {
    SetMapEntry *e = m->buckets[i];
    while (e) {
      SetMapEntry *next = e->next;
      free(e->key);
      free(e->ranges);
      free(e);
      e = next;
    }
  }
}

// ============================================================================
// LineRangeSet — set of line ranges with add/subtract/intersect/contains
// ============================================================================
// Stores a sorted list of non-overlapping line ranges.

typedef struct {
  LineRange *ranges;
  int count;
  int cap;
} LineRangeSet;

static void lrs_init(LineRangeSet *s) {
  s->ranges = NULL;
  s->count = 0;
  s->cap = 0;
}
static void lrs_free(LineRangeSet *s) { free(s->ranges); }

static void lrs_add_range(LineRangeSet *s, LineRange r) {
  if (lr_length(r) <= 0)
    return;
  // Find insertion position (sorted by start_line)
  int pos = 0;
  while (pos < s->count && s->ranges[pos].start_line < r.start_line)
    pos++;

  // Merge with overlapping/adjacent ranges
  int merge_start = pos;
  while (merge_start > 0 && s->ranges[merge_start - 1].end_line >= r.start_line)
    merge_start--;
  int merge_end = pos;
  while (merge_end < s->count && s->ranges[merge_end].start_line <= r.end_line)
    merge_end++;

  if (merge_start < merge_end) {
    int min_s = s->ranges[merge_start].start_line < r.start_line ? s->ranges[merge_start].start_line
                                                                 : r.start_line;
    int max_e = s->ranges[merge_end - 1].end_line > r.end_line ? s->ranges[merge_end - 1].end_line
                                                               : r.end_line;
    LineRange merged = lr_new(min_s, max_e);
    // Remove [merge_start, merge_end) and insert merged at merge_start
    int removed = merge_end - merge_start;
    if (removed > 1) {
      memmove(&s->ranges[merge_start + 1], &s->ranges[merge_end],
              (size_t)(s->count - merge_end) * sizeof(LineRange));
      s->count -= (removed - 1);
    }
    s->ranges[merge_start] = merged;
  } else {
    // Insert new range at pos
    if (s->count >= s->cap) {
      s->cap = s->cap == 0 ? 8 : s->cap * 2;
      s->ranges = (LineRange *)realloc(s->ranges, (size_t)s->cap * sizeof(LineRange));
    }
    memmove(&s->ranges[pos + 1], &s->ranges[pos], (size_t)(s->count - pos) * sizeof(LineRange));
    s->ranges[pos] = r;
    s->count++;
  }
}

static bool lrs_contains(const LineRangeSet *s, int line) {
  // Binary search for range containing line
  int lo = 0, hi = s->count - 1;
  while (lo <= hi) {
    int mid = (lo + hi) / 2;
    if (s->ranges[mid].end_line <= line)
      lo = mid + 1;
    else if (s->ranges[mid].start_line > line)
      hi = mid - 1;
    else
      return true;
  }
  return false;
}

// subtractFrom: given a range r, return the parts of r NOT covered by the set.
// Returns a dynamic array of LineRange (caller must free).
typedef struct {
  LineRange *ranges;
  int count;
} LineRangeArray;

static LineRangeArray lrs_subtract_from(const LineRangeSet *s, LineRange r) {
  LineRangeArray result;
  result.ranges = NULL;
  result.count = 0;
  int cap = 0;

  int current_start = r.start_line;
  for (int i = 0; i < s->count; i++) {
    if (s->ranges[i].end_line <= current_start)
      continue;
    if (s->ranges[i].start_line >= r.end_line)
      break;
    int gap_end = s->ranges[i].start_line < r.end_line ? s->ranges[i].start_line : r.end_line;
    if (gap_end > current_start) {
      if (result.count >= cap) {
        cap = cap == 0 ? 4 : cap * 2;
        result.ranges = (LineRange *)realloc(result.ranges, (size_t)cap * sizeof(LineRange));
      }
      result.ranges[result.count++] = lr_new(current_start, gap_end);
    }
    current_start = s->ranges[i].end_line;
  }
  if (current_start < r.end_line) {
    if (result.count >= cap) {
      cap = cap == 0 ? 4 : cap * 2;
      result.ranges = (LineRange *)realloc(result.ranges, (size_t)cap * sizeof(LineRange));
    }
    result.ranges[result.count++] = lr_new(current_start, r.end_line);
  }
  return result;
}

// getWithDelta: shift all ranges by delta
static LineRangeArray lra_with_delta(LineRangeArray a, int delta) {
  for (int i = 0; i < a.count; i++) {
    a.ranges[i].start_line += delta;
    a.ranges[i].end_line += delta;
  }
  return a;
}

// getIntersection of two LineRangeArrays
static LineRangeArray lra_intersect(LineRangeArray a, LineRangeArray b) {
  LineRangeArray result;
  result.ranges = NULL;
  result.count = 0;
  int cap = 0;
  int j = 0;
  for (int i = 0; i < a.count; i++) {
    while (j < b.count && b.ranges[j].end_line <= a.ranges[i].start_line)
      j++;
    int k = j;
    while (k < b.count && b.ranges[k].start_line < a.ranges[i].end_line) {
      int s = a.ranges[i].start_line > b.ranges[k].start_line ? a.ranges[i].start_line
                                                              : b.ranges[k].start_line;
      int e =
          a.ranges[i].end_line < b.ranges[k].end_line ? a.ranges[i].end_line : b.ranges[k].end_line;
      if (s < e) {
        if (result.count >= cap) {
          cap = cap == 0 ? 4 : cap * 2;
          result.ranges = (LineRange *)realloc(result.ranges, (size_t)cap * sizeof(LineRange));
        }
        result.ranges[result.count++] = lr_new(s, e);
      }
      k++;
    }
  }
  return result;
}

// ============================================================================
// MonotonousArray — binary search for sorted changes array
// ============================================================================

static int find_last_monotonous_idx(const DetailedLineRangeMapping *arr, int len,
                                    bool (*pred)(const DetailedLineRangeMapping *, int), int value,
                                    int start_idx) {
  (void)pred;
  int lo = start_idx, hi = len;
  while (lo < hi) {
    int mid = (lo + hi) / 2;
    if (arr[mid].original.start_line <= value)
      lo = mid + 1;
    else
      hi = mid;
  }
  return lo - 1;
}

static int find_last_idx_by_orig_start_lt(const DetailedLineRangeMapping *arr, int len, int value,
                                          int start_idx) {
  int lo = start_idx, hi = len;
  while (lo < hi) {
    int mid = (lo + hi) / 2;
    if (arr[mid].original.start_line < value)
      lo = mid + 1;
    else
      hi = mid;
  }
  return lo - 1;
}

static int find_last_idx_by_mod_start_le(const DetailedLineRangeMapping *arr, int len, int value) {
  int lo = 0, hi = len;
  while (lo < hi) {
    int mid = (lo + hi) / 2;
    if (arr[mid].modified.start_line <= value)
      lo = mid + 1;
    else
      hi = mid;
  }
  return lo - 1;
}

static int find_last_idx_by_mod_start_lt(const DetailedLineRangeMapping *arr, int len, int value) {
  int lo = 0, hi = len;
  while (lo < hi) {
    int mid = (lo + hi) / 2;
    if (arr[mid].modified.start_line < value)
      lo = mid + 1;
    else
      hi = mid;
  }
  return lo - 1;
}

// ============================================================================
// isSpace — matches VSCode's isSpace(charCode) from utils.ts
// ============================================================================

static bool is_space(unsigned char ch) { return ch == ' ' || ch == '\t'; }

// ============================================================================
// areLinesSimilar — character-level similarity using Myers diff
// ============================================================================
// VSCode: >0.6 ratio of common non-space chars AND >10 non-space chars

static bool are_lines_similar(const char *line1, const char *line2, MoveTimeout *timeout) {
  // Trim compare
  char *t1 = trim_string(line1);
  char *t2 = trim_string(line2);
  if (strcmp(t1, t2) == 0) {
    free(t1);
    free(t2);
    return true;
  }
  free(t1);
  free(t2);

  int len1 = (int)strlen(line1);
  int len2 = (int)strlen(line2);
  if (len1 > 300 && len2 > 300)
    return false;

  // Build char sequences for Myers diff
  // VSCode: new LinesSliceCharSequence([line1], new Range(1,1,1,line1.length), false)
  // Note: VSCode uses line.length as endCol (1-based column), which truncates by 1 char
  // compared to the full trimmed line. We must replicate this exactly.
  CharRange r1 = {1, 1, 1, len1};
  CharRange r2 = {1, 1, 1, len2};
  const char *lines1[1] = {line1};
  const char *lines2[1] = {line2};
  ISequence *seq1 = char_sequence_create_from_range(lines1, 1, &r1, false);
  ISequence *seq2 = char_sequence_create_from_range(lines2, 1, &r2, false);

  if (!seq1 || !seq2) {
    if (seq1)
      seq1->destroy(seq1);
    if (seq2)
      seq2->destroy(seq2);
    return false;
  }

  // Run Myers diff
  bool hit_timeout = false;
  int remaining_ms = 0;
  if (timeout->timeout_ms > 0) {
    int64_t elapsed = get_current_time_ms() - timeout->start_time_ms;
    remaining_ms = timeout->timeout_ms - (int)elapsed;
    if (remaining_ms <= 0) {
      seq1->destroy(seq1);
      seq2->destroy(seq2);
      return false;
    }
  }

  SequenceDiffArray *diffs;
  diffs = myers_nd_diff_algorithm(seq1, seq2, remaining_ms, &hit_timeout);

  if (!diffs) {
    seq1->destroy(seq1);
    seq2->destroy(seq2);
    return false;
  }

  // Invert diffs to get matching regions (SequenceDiff.invert)
  // VSCode quirk: invert uses line1.length (original string length) as doc1Length,
  // not seq1.length. The counting loop then uses line1.charCodeAt(idx) on the
  // original string. This means matching regions extend beyond the sequence and
  // the index-to-character mapping is offset by trimmed whitespace. We replicate
  // this exactly for parity.
  int common_non_space = 0;
  int prev_end = 0;
  for (int i = 0; i < diffs->count; i++) {
    int match_start = prev_end;
    int match_end = diffs->diffs[i].seq1_start;
    for (int idx = match_start; idx < match_end; idx++) {
      if (idx < len1 && !is_space((unsigned char)line1[idx])) {
        common_non_space++;
      }
    }
    prev_end = diffs->diffs[i].seq1_end;
  }
  // Last matching segment — extends to len1 (line1.length), not s1len
  for (int idx = prev_end; idx < len1; idx++) {
    if (!is_space((unsigned char)line1[idx])) {
      common_non_space++;
    }
  }

  // Count non-ws chars in longer line
  // VSCode bug: countNonWsChars always iterates over line1.length chars
  // regardless of which string is passed. We replicate this exactly.
  int non_ws_count = 0;
  const char *longer_line = len1 > len2 ? line1 : line2;
  // VSCode iterates i < line1.length on the passed string
  // But uses str.charCodeAt(i) — so if str is line2 but len is line1.length,
  // it can read beyond line2. However, in practice the longer line is chosen
  // so line1.length <= longer_line.length when line1 is longer.
  // Actually, re-reading VSCode: it always iterates `i < line1.length`
  // This is a bug in VSCode but we must replicate it for parity.
  for (int i = 0; i < len1; i++) {
    unsigned char ch = (unsigned char)longer_line[i];
    if (!is_space(ch)) {
      non_ws_count++;
    }
  }

  bool result = (non_ws_count > 10) && ((double)common_non_space / (double)non_ws_count > 0.6);

  sequence_diff_array_free(diffs);
  seq1->destroy(seq1);
  seq2->destroy(seq2);
  return result;
}

// ============================================================================
// computeMovesFromSimpleDeletionsToSimpleInsertions
// ============================================================================

typedef struct {
  MoveArray moves;
  bool *excluded; // indexed by change index; true if excluded
} SimpleMovesResult;

static SimpleMovesResult compute_simple_moves(const DetailedLineRangeMapping *changes,
                                              int change_count, const char **original_lines,
                                              const char **modified_lines, MoveTimeout *timeout) {
  SimpleMovesResult result;
  ma_init(&result.moves);
  result.excluded = (bool *)calloc((size_t)change_count, sizeof(bool));

  // Build deletions and insertions
  int del_count = 0, ins_count = 0;
  for (int i = 0; i < change_count; i++) {
    if (lr_is_empty(changes[i].modified) && lr_length(changes[i].original) >= 3)
      del_count++;
    if (lr_is_empty(changes[i].original) && lr_length(changes[i].modified) >= 3)
      ins_count++;
  }

  if (del_count == 0 || ins_count == 0)
    return result;

  // Build deletion fragments
  int *del_indices = (int *)malloc((size_t)del_count * sizeof(int));
  LineRangeFragment *deletions =
      (LineRangeFragment *)malloc((size_t)del_count * sizeof(LineRangeFragment));
  int di = 0;
  for (int i = 0; i < change_count; i++) {
    if (lr_is_empty(changes[i].modified) && lr_length(changes[i].original) >= 3) {
      del_indices[di] = i;
      deletions[di] = lrf_create(changes[i].original, original_lines, i);
      di++;
    }
  }

  // Build insertion fragments
  int *ins_indices = (int *)malloc((size_t)ins_count * sizeof(int));
  LineRangeFragment *insertions =
      (LineRangeFragment *)malloc((size_t)ins_count * sizeof(LineRangeFragment));
  bool *ins_used = (bool *)calloc((size_t)ins_count, sizeof(bool));
  int ii = 0;
  for (int i = 0; i < change_count; i++) {
    if (lr_is_empty(changes[i].original) && lr_length(changes[i].modified) >= 3) {
      ins_indices[ii] = i;
      insertions[ii] = lrf_create(changes[i].modified, modified_lines, i);
      ii++;
    }
  }

  // Match deletions to insertions
  for (int d = 0; d < del_count; d++) {
    double highest = -1.0;
    int best = -1;
    for (int j = 0; j < ins_count; j++) {
      if (ins_used[j])
        continue;
      double sim = lrf_compute_similarity(&deletions[d], &insertions[j]);
      if (sim > highest) {
        highest = sim;
        best = j;
      }
    }
    if (highest > 0.90 && best >= 0) {
      ins_used[best] = true;
      MovedText m;
      m.original = deletions[d].range;
      m.modified = insertions[best].range;
      ma_push(&result.moves, m);
      result.excluded[del_indices[d]] = true;
      result.excluded[ins_indices[best]] = true;
    }
    if (!timeout_is_valid(timeout))
      break;
  }

  // Cleanup
  for (int i = 0; i < del_count; i++)
    lrf_free(&deletions[i]);
  for (int i = 0; i < ins_count; i++)
    lrf_free(&insertions[i]);
  free(deletions);
  free(insertions);
  free(del_indices);
  free(ins_indices);
  free(ins_used);

  return result;
}

// ============================================================================
// Comparison for sorting
// ============================================================================

static int cmp_by_mod_start(const void *a, const void *b) {
  const DetailedLineRangeMapping *ma = (const DetailedLineRangeMapping *)a;
  const DetailedLineRangeMapping *mb = (const DetailedLineRangeMapping *)b;
  return ma->modified.start_line - mb->modified.start_line;
}

// ============================================================================
// PossibleMapping for computeUnchangedMoves
// ============================================================================

typedef struct {
  LineRange modified_range;
  LineRange original_range;
} PossibleMapping;

typedef struct {
  PossibleMapping *items;
  int count;
  int cap;
} PossibleMappingArray;

static void pma_push(PossibleMappingArray *a, PossibleMapping m) {
  if (a->count >= a->cap) {
    a->cap = a->cap == 0 ? 16 : a->cap * 2;
    a->items = (PossibleMapping *)realloc(a->items, (size_t)a->cap * sizeof(PossibleMapping));
  }
  a->items[a->count++] = m;
}

// Sort possible mappings by modified range length descending (reverseOrder)
static int cmp_pm_by_length_desc(const void *a, const void *b) {
  const PossibleMapping *pa = (const PossibleMapping *)a;
  const PossibleMapping *pb = (const PossibleMapping *)b;
  int la = lr_length(pa->modified_range);
  int lb = lr_length(pb->modified_range);
  return lb - la;
}

// ============================================================================
// computeUnchangedMoves
// ============================================================================

static void compute_unchanged_moves(const DetailedLineRangeMapping *changes, int change_count,
                                    const uint32_t *hashed_original,
                                    const uint32_t *hashed_modified, const char **original_lines,
                                    int original_count, const char **modified_lines,
                                    int modified_count, MoveTimeout *timeout,
                                    MoveArray *out_moves) {
  // Build 3-line hash map from original changes
  SetMap original3;
  setmap_init(&original3);

  // Helper: write a uint32 as decimal into buf, return chars written
  #define WRITE_U32(buf, pos, val) do { \
    unsigned _v = (unsigned)(val); \
    char _tmp[11]; int _n = 0; \
    if (_v == 0) { _tmp[_n++] = '0'; } \
    else { while (_v) { _tmp[_n++] = '0' + (_v % 10); _v /= 10; } } \
    for (int _j = _n - 1; _j >= 0; _j--) (buf)[(pos)++] = _tmp[_j]; \
  } while (0)

  char key_buf[64];
  for (int ci = 0; ci < change_count; ci++) {
    LineRange orig = changes[ci].original;
    for (int i = orig.start_line; i < orig.end_line - 2; i++) {
      int pos = 0;
      WRITE_U32(key_buf, pos, hashed_original[i - 1]);
      key_buf[pos++] = ':';
      WRITE_U32(key_buf, pos, hashed_original[i]);
      key_buf[pos++] = ':';
      WRITE_U32(key_buf, pos, hashed_original[i + 1]);
      key_buf[pos] = '\0';
      setmap_add(&original3, key_buf, lr_new(i, i + 3));
    }
  }

  // Sort changes by modified start (we need a mutable copy)
  DetailedLineRangeMapping *sorted_changes =
      (DetailedLineRangeMapping *)malloc((size_t)change_count * sizeof(DetailedLineRangeMapping));
  memcpy(sorted_changes, changes, (size_t)change_count * sizeof(DetailedLineRangeMapping));
  qsort(sorted_changes, (size_t)change_count, sizeof(DetailedLineRangeMapping), cmp_by_mod_start);

  // Find possible mappings using 3-line sliding window
  PossibleMappingArray possible;
  possible.items = NULL;
  possible.count = 0;
  possible.cap = 0;

  // For extending last mappings, we track active mappings per iteration
  typedef struct {
    int pm_idx;
  } ActiveMapping;
  ActiveMapping *last_mappings = NULL;
  int last_count = 0, last_cap = 0;
  ActiveMapping *next_mappings = NULL;
  int next_count = 0, next_cap = 0;

  for (int ci = 0; ci < change_count; ci++) {
    LineRange mod = sorted_changes[ci].modified;
    last_count = 0;

    for (int i = mod.start_line; i < mod.end_line - 2; i++) {
      int pos = 0;
      WRITE_U32(key_buf, pos, hashed_modified[i - 1]);
      key_buf[pos++] = ':';
      WRITE_U32(key_buf, pos, hashed_modified[i]);
      key_buf[pos++] = ':';
      WRITE_U32(key_buf, pos, hashed_modified[i + 1]);
      key_buf[pos] = '\0';
      LineRange current_mod = lr_new(i, i + 3);

      next_count = 0;
      SetMapEntry *entry = setmap_find(&original3, key_buf);
      if (entry) {
        for (int ri = 0; ri < entry->count; ri++) {
          LineRange orig_range = entry->ranges[ri];
          bool extended = false;

          // Check if this extends a previous mapping
          for (int li = 0; li < last_count; li++) {
            PossibleMapping *lm = &possible.items[last_mappings[li].pm_idx];
            if (lm->original_range.end_line + 1 == orig_range.end_line &&
                lm->modified_range.end_line + 1 == current_mod.end_line) {
              lm->original_range = lr_new(lm->original_range.start_line, orig_range.end_line);
              lm->modified_range = lr_new(lm->modified_range.start_line, current_mod.end_line);
              // Push to next
              if (next_count >= next_cap) {
                next_cap = next_cap == 0 ? 8 : next_cap * 2;
                next_mappings = (ActiveMapping *)realloc(next_mappings,
                                                         (size_t)next_cap * sizeof(ActiveMapping));
              }
              next_mappings[next_count++] = last_mappings[li];
              extended = true;
              break;
            }
          }

          if (!extended) {
            PossibleMapping pm;
            pm.modified_range = current_mod;
            pm.original_range = orig_range;
            pma_push(&possible, pm);
            if (next_count >= next_cap) {
              next_cap = next_cap == 0 ? 8 : next_cap * 2;
              next_mappings =
                  (ActiveMapping *)realloc(next_mappings, (size_t)next_cap * sizeof(ActiveMapping));
            }
            next_mappings[next_count++] = (ActiveMapping){possible.count - 1};
          }
        }
      }

      // Swap last/next
      ActiveMapping *tmp = last_mappings;
      int tc = last_cap;
      last_mappings = next_mappings;
      last_count = next_count;
      last_cap = next_cap;
      next_mappings = tmp;
      next_count = 0;
      next_cap = tc;
    }

    if (!timeout_is_valid(timeout)) {
      free(last_mappings);
      free(next_mappings);
      free(possible.items);
      free(sorted_changes);
      setmap_free(&original3);
      return;
    }
  }

  free(last_mappings);
  free(next_mappings);

  // Sort by modified range length descending
  qsort(possible.items, (size_t)possible.count, sizeof(PossibleMapping), cmp_pm_by_length_desc);

  LineRangeSet modified_set, original_set;
  lrs_init(&modified_set);
  lrs_init(&original_set);

  MoveArray moves;
  ma_init(&moves);

  for (int pi = 0; pi < possible.count; pi++) {
    PossibleMapping *pm = &possible.items[pi];
    int diff_orig_to_mod = pm->modified_range.start_line - pm->original_range.start_line;

    LineRangeArray mod_sections = lrs_subtract_from(&modified_set, pm->modified_range);
    LineRangeArray orig_sections = lrs_subtract_from(&original_set, pm->original_range);
    LineRangeArray orig_translated = lra_with_delta(orig_sections, diff_orig_to_mod);
    LineRangeArray intersected = lra_intersect(mod_sections, orig_translated);

    for (int si = 0; si < intersected.count; si++) {
      LineRange s = intersected.ranges[si];
      if (lr_length(s) < 3)
        continue;
      LineRange mod_lr = s;
      LineRange orig_lr = lr_delta(s, -diff_orig_to_mod);

      MovedText m = {orig_lr, mod_lr};
      ma_push(&moves, m);
      lrs_add_range(&modified_set, mod_lr);
      lrs_add_range(&original_set, orig_lr);
    }

    free(mod_sections.ranges);
    free(orig_translated.ranges);
    free(intersected.ranges);
  }

  // Sort moves by original start
  for (int i = 0; i < moves.count - 1; i++) {
    for (int j = i + 1; j < moves.count; j++) {
      if (moves.items[j].original.start_line < moves.items[i].original.start_line) {
        MovedText tmp = moves.items[i];
        moves.items[i] = moves.items[j];
        moves.items[j] = tmp;
      }
    }
  }

  // Extend moves using areLinesSimilar
  // Use the original unsorted changes for findLastMonotonous lookups
  // (changes must be sorted by original.start_line for MonotonousArray — they already are)
  int mono_last_idx = 0;
  for (int mi = 0; mi < moves.count; mi++) {
    MovedText *mv = &moves.items[mi];

    // Find first touching change for original
    int ft_orig_idx = find_last_monotonous_idx(changes, change_count, NULL, mv->original.start_line,
                                               mono_last_idx);
    mono_last_idx = ft_orig_idx + 1;

    int ft_mod_idx = find_last_idx_by_mod_start_le(changes, change_count, mv->modified.start_line);

    int lines_above = 0;
    if (ft_orig_idx >= 0) {
      int a = mv->original.start_line - changes[ft_orig_idx].original.start_line;
      int b = (ft_mod_idx >= 0)
                  ? (mv->modified.start_line - changes[ft_mod_idx].modified.start_line)
                  : 0;
      lines_above = a > b ? a : b;
    }

    int lt_orig_idx =
        find_last_idx_by_orig_start_lt(changes, change_count, mv->original.end_line, 0);
    int lt_mod_idx = find_last_idx_by_mod_start_lt(changes, change_count, mv->modified.end_line);

    int lines_below = 0;
    if (lt_orig_idx >= 0) {
      int a = changes[lt_orig_idx].original.end_line - mv->original.end_line;
      int b =
          (lt_mod_idx >= 0) ? (changes[lt_mod_idx].modified.end_line - mv->modified.end_line) : 0;
      lines_below = a > b ? a : b;
    }

    // Extend upward
    int extend_top = 0;
    for (extend_top = 0; extend_top < lines_above; extend_top++) {
      int orig_line = mv->original.start_line - extend_top - 1;
      int mod_line = mv->modified.start_line - extend_top - 1;
      if (orig_line > original_count || mod_line > modified_count)
        break;
      if (orig_line < 1 || mod_line < 1)
        break;
      if (lrs_contains(&modified_set, mod_line) || lrs_contains(&original_set, orig_line))
        break;
      if (!are_lines_similar(original_lines[orig_line - 1], modified_lines[mod_line - 1], timeout))
        break;
    }
    if (extend_top > 0) {
      lrs_add_range(&original_set,
                    lr_new(mv->original.start_line - extend_top, mv->original.start_line));
      lrs_add_range(&modified_set,
                    lr_new(mv->modified.start_line - extend_top, mv->modified.start_line));
    }

    // Extend downward
    int extend_bottom = 0;
    for (extend_bottom = 0; extend_bottom < lines_below; extend_bottom++) {
      int orig_line = mv->original.end_line + extend_bottom;
      int mod_line = mv->modified.end_line + extend_bottom;
      if (orig_line > original_count || mod_line > modified_count)
        break;
      if (lrs_contains(&modified_set, mod_line) || lrs_contains(&original_set, orig_line))
        break;
      if (!are_lines_similar(original_lines[orig_line - 1], modified_lines[mod_line - 1], timeout))
        break;
    }
    if (extend_bottom > 0) {
      lrs_add_range(&original_set,
                    lr_new(mv->original.end_line, mv->original.end_line + extend_bottom));
      lrs_add_range(&modified_set,
                    lr_new(mv->modified.end_line, mv->modified.end_line + extend_bottom));
    }

    if (extend_top > 0 || extend_bottom > 0) {
      mv->original =
          lr_new(mv->original.start_line - extend_top, mv->original.end_line + extend_bottom);
      mv->modified =
          lr_new(mv->modified.start_line - extend_top, mv->modified.end_line + extend_bottom);
    }
  }

  // Copy results
  for (int i = 0; i < moves.count; i++) {
    ma_push(out_moves, moves.items[i]);
  }

  // Cleanup
  ma_free(&moves);
  lrs_free(&modified_set);
  lrs_free(&original_set);
  free(possible.items);
  free(sorted_changes);
  setmap_free(&original3);
}

// ============================================================================
// joinCloseConsecutiveMoves
// ============================================================================

static int cmp_move_by_orig_start(const void *a, const void *b) {
  const MovedText *ma = (const MovedText *)a;
  const MovedText *mb = (const MovedText *)b;
  return ma->original.start_line - mb->original.start_line;
}

static MoveArray join_close_consecutive_moves(MoveArray *moves) {
  MoveArray result;
  ma_init(&result);
  if (moves->count == 0)
    return result;

  qsort(moves->items, (size_t)moves->count, sizeof(MovedText), cmp_move_by_orig_start);

  ma_push(&result, moves->items[0]);
  for (int i = 1; i < moves->count; i++) {
    MovedText *last = &result.items[result.count - 1];
    MovedText *current = &moves->items[i];

    int orig_dist = current->original.start_line - last->original.end_line;
    int mod_dist = current->modified.start_line - last->modified.end_line;
    bool after_last = (orig_dist >= 0 && mod_dist >= 0);

    if (after_last && orig_dist + mod_dist <= 2) {
      // Join: LineRangeMapping.join = union of both ranges
      last->original = lr_join(last->original, current->original);
      last->modified = lr_join(last->modified, current->modified);
      continue;
    }
    ma_push(&result, *current);
  }
  return result;
}

// ============================================================================
// removeMovesInSameDiff
// ============================================================================

static MoveArray remove_moves_in_same_diff(const DetailedLineRangeMapping *changes,
                                           int change_count, MoveArray *moves) {
  MoveArray result;
  ma_init(&result);

  int mono_idx = 0;
  for (int i = 0; i < moves->count; i++) {
    MovedText *m = &moves->items[i];

    // findLastMonotonous for original.endExclusive
    int orig_idx = find_last_idx_by_orig_start_lt(changes, change_count, m->original.end_line, 0);
    if (orig_idx < 0) {
      // No change before — treat as default LineRangeMapping(1,1,1,1)
      orig_idx = -1;
    }

    int mod_idx = find_last_idx_by_mod_start_lt(changes, change_count, m->modified.end_line);

    // VSCode: diffBeforeEndOfMoveOriginal !== diffBeforeEndOfMoveModified
    // If they point to different changes (or one is the default), it's valid
    bool different;
    if (orig_idx < 0 && mod_idx < 0) {
      different = false;
    } else if (orig_idx < 0 || mod_idx < 0) {
      different = true;
    } else {
      different = (orig_idx != mod_idx);
    }

    if (different) {
      ma_push(&result, *m);
    }
  }
  (void)mono_idx;
  return result;
}

// ============================================================================
// countWhere utility
// ============================================================================

static int count_where_len_ge2(const char **lines, int start_line, int end_line) {
  int count = 0;
  for (int i = start_line - 1; i < end_line - 1; i++) {
    const char *line = lines[i];
    // Trim and check length >= 2
    const char *s = line;
    while (*s == ' ' || *s == '\t' || *s == '\r' || *s == '\n')
      s++;
    const char *e = line + strlen(line);
    while (e > s && (*(e - 1) == ' ' || *(e - 1) == '\t' || *(e - 1) == '\r' || *(e - 1) == '\n'))
      e--;
    if (e - s >= 2)
      count++;
  }
  return count;
}

// ============================================================================
// Main entry point: compute_moved_lines
// ============================================================================

void compute_moved_lines(const DetailedLineRangeMapping *changes, int change_count,
                         const char **original_lines, int original_count,
                         const char **modified_lines, int modified_count,
                         const uint32_t *hashed_original, const uint32_t *hashed_modified,
                         int timeout_ms, MovedTextArray *out_moves) {
  out_moves->moves = NULL;
  out_moves->count = 0;
  out_moves->capacity = 0;

  if (change_count == 0)
    return;

  MoveTimeout timeout;
  timeout.timeout_ms = timeout_ms;
  timeout.start_time_ms = get_current_time_ms();

  // Step 1: Simple deletion-to-insertion moves
  SimpleMovesResult simple =
      compute_simple_moves(changes, change_count, original_lines, modified_lines, &timeout);

  if (!timeout_is_valid(&timeout)) {
    ma_free(&simple.moves);
    free(simple.excluded);
    return;
  }

  // Step 2: Filter excluded changes and compute unchanged moves
  int filtered_count = 0;
  for (int i = 0; i < change_count; i++) {
    if (!simple.excluded[i])
      filtered_count++;
  }

  DetailedLineRangeMapping *filtered = (DetailedLineRangeMapping *)malloc(
      (size_t)(filtered_count > 0 ? filtered_count : 1) * sizeof(DetailedLineRangeMapping));
  int fi = 0;
  for (int i = 0; i < change_count; i++) {
    if (!simple.excluded[i]) {
      filtered[fi++] = changes[i];
    }
  }

  MoveArray unchanged_moves;
  ma_init(&unchanged_moves);
  compute_unchanged_moves(filtered, filtered_count, hashed_original, hashed_modified,
                          original_lines, original_count, modified_lines, modified_count, &timeout,
                          &unchanged_moves);

  // Combine moves
  MoveArray all_moves;
  ma_init(&all_moves);
  for (int i = 0; i < simple.moves.count; i++)
    ma_push(&all_moves, simple.moves.items[i]);
  for (int i = 0; i < unchanged_moves.count; i++)
    ma_push(&all_moves, unchanged_moves.items[i]);

  // Step 3: Join close consecutive moves
  MoveArray joined = join_close_consecutive_moves(&all_moves);

  // Step 4: Filter too-short moves
  // original text must be >= 15 chars AND >= 2 lines with trimmed length >= 2
  MoveArray filtered_moves;
  ma_init(&filtered_moves);
  for (int i = 0; i < joined.count; i++) {
    MovedText *m = &joined.items[i];
    // Build trimmed text of original lines
    int total_len = 0;
    for (int line = m->original.start_line; line < m->original.end_line; line++) {
      const char *l = original_lines[line - 1];
      const char *s = l;
      while (*s == ' ' || *s == '\t' || *s == '\r' || *s == '\n')
        s++;
      const char *e = l + strlen(l);
      while (e > s && (*(e - 1) == ' ' || *(e - 1) == '\t' || *(e - 1) == '\r' || *(e - 1) == '\n'))
        e--;
      total_len += (int)(e - s);
      if (line < m->original.end_line - 1)
        total_len++; // \n between lines
    }
    int count_ge2 =
        count_where_len_ge2(original_lines, m->original.start_line, m->original.end_line);
    if (total_len >= 15 && count_ge2 >= 2) {
      ma_push(&filtered_moves, *m);
    }
  }

  // Step 5: Remove moves in same diff
  MoveArray final_moves = remove_moves_in_same_diff(changes, change_count, &filtered_moves);

  // Output
  if (final_moves.count > 0) {
    out_moves->moves = final_moves.items;
    out_moves->count = final_moves.count;
    out_moves->capacity = final_moves.cap;
  } else {
    ma_free(&final_moves);
  }

  // Cleanup
  ma_free(&simple.moves);
  free(simple.excluded);
  free(filtered);
  ma_free(&unchanged_moves);
  ma_free(&all_moves);
  ma_free(&joined);
  ma_free(&filtered_moves);
}
