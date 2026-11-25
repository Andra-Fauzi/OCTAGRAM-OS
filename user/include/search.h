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

#ifdef __cplusplus
}
#endif
#endif
