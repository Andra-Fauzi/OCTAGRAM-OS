#include "test.h"
#include <float.h>
#include <stdalign.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdnoreturn.h>

int main() {
  ASSERT(16, sizeof(long double));
  ASSERT(16, __SIZEOF_LONG_DOUBLE__);
  ASSERT(64, LDBL_MANT_DIG);
  ASSERT(16384, LDBL_MAX_EXP);
  printf("OK\n");
  return 0;
}
