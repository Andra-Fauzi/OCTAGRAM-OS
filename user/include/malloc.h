#ifndef OCTAGRAM_MALLOC_H
#define OCTAGRAM_MALLOC_H

#include <stddef.h>
#include <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The OCTAGRAM allocator has no tunable thresholds, so mallopt reports failure
 * (0) for every parameter instead of pretending a value was applied. */
#define M_TRIM_THRESHOLD (-1)
#define M_TOP_PAD (-2)
#define M_MMAP_THRESHOLD (-3)
#define M_MMAP_MAX (-4)
#define M_CHECK_ACTION (-5)
#define M_PERTURB (-6)
#define M_ARENA_MAX (-8)

int mallopt(int parameter, int value);

/* Nothing is returned to the system: the allocator keeps its arena, so this
 * reports 0 (no memory released). */
int malloc_trim(size_t pad);

/* Prints a single honest line to stderr; per-arena statistics are not tracked. */
void malloc_stats(void);

#ifdef __cplusplus
}
#endif
#endif
