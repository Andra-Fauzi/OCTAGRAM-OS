#include <stdint.h>

_Static_assert(sizeof(int_least8_t) == 1, "int_least8_t width");
_Static_assert(sizeof(uint_least8_t) == 1, "uint_least8_t width");
_Static_assert(sizeof(int_least16_t) == 2, "int_least16_t width");
_Static_assert(sizeof(uint_least16_t) == 2, "uint_least16_t width");
_Static_assert(sizeof(int_least32_t) == 4, "int_least32_t width");
_Static_assert(sizeof(uint_least32_t) == 4, "uint_least32_t width");
_Static_assert(sizeof(int_least64_t) == 8, "int_least64_t width");
_Static_assert(sizeof(uint_least64_t) == 8, "uint_least64_t width");

_Static_assert(sizeof(int_fast8_t) >= 1, "int_fast8_t minimum width");
_Static_assert(sizeof(uint_fast16_t) >= 2, "uint_fast16_t minimum width");
_Static_assert(sizeof(int_fast32_t) >= 4, "int_fast32_t minimum width");
_Static_assert(sizeof(uint_fast64_t) >= 8, "uint_fast64_t minimum width");

_Static_assert(INT_LEAST8_MIN == INT8_MIN, "least-width limits");
_Static_assert(UINT_LEAST64_MAX == UINT64_MAX, "least-width unsigned limits");
_Static_assert(INT_FAST32_MAX == INT64_MAX, "fast-width limits");
_Static_assert(UINT_FAST8_MAX == UINT64_MAX, "fast-width unsigned limits");

int main(void) { return 0; }
