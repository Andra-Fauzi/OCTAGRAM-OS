#ifndef OCTAGRAM_SEARCH_H
#define OCTAGRAM_SEARCH_H

#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Binary search tree. tsearch keeps the tree AVL balanced, so lookups stay
 * logarithmic even when keys are inserted in sorted order. Nodes own the key
 * pointer, not the key itself. */
typedef enum { preorder, postorder, endorder, leaf } VISIT;

void *tsearch(const void *key, void **root_pointer,
              int (*compar)(const void *, const void *));
void *tfind(const void *key, void *const *root_pointer,
            int (*compar)(const void *, const void *));
void *tdelete(const void *key, void **root_pointer,
              int (*compar)(const void *, const void *));
void twalk(const void *root, void (*action)(const void *, VISIT, int));
void tdestroy(void *root, void (*free_function)(void *));

/* Intrusive doubly linked list whose links live in the first two words of the
 * element (the historic struct qelem layout). */
struct qelem {
    struct qelem *q_forw;
    struct qelem *q_back;
    char q_data[];
};

void insque(void *element, void *predecessor);
void remque(void *element);

/* Hash table. Keys are borrowed: hdestroy frees the table, not the strings.
 * hcreate/hsearch/hdestroy use one process-wide table; the _r variants keep the
 * table in the caller's struct and are safe to use side by side. */
typedef struct entry {
    char *key;
    void *data;
} ENTRY;

typedef enum { FIND, ENTER } ACTION;

struct hsearch_data {
    size_t capacity;
    size_t count;
    struct hsearch_node **buckets;
};

int hcreate(size_t element_count);
void hdestroy(void);
ENTRY *hsearch(ENTRY item, ACTION action);
int hcreate_r(size_t element_count, struct hsearch_data *table);
void hdestroy_r(struct hsearch_data *table);
int hsearch_r(ENTRY item, ACTION action, ENTRY **result,
              struct hsearch_data *table);

#ifdef __cplusplus
}
#endif
#endif
