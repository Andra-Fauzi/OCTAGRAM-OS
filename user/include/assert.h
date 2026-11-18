#ifndef OCTAGRAM_ASSERT_H
#define OCTAGRAM_ASSERT_H

#include <stdlib.h>

#ifdef NDEBUG
#define assert(expression) ((void)0)
#else
#define assert(expression) ((expression) ? (void)0 : abort())
#endif

#endif
