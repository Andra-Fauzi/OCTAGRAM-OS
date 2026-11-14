#ifndef OCTAGRAM_ASSERT_H
#define OCTAGRAM_ASSERT_H

#include <stdlib.h>

#define assert(expression) ((expression) ? (void)0 : abort())

#endif
