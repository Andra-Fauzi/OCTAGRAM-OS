/* OCTAGRAM's stdint.h omits the C99 "least"/"fast" integer typedefs. */
#ifndef OCTAGRAM_POC_FIX_H
#define OCTAGRAM_POC_FIX_H
typedef signed char int_least8_t;
typedef unsigned char uint_least8_t;
typedef short int_least16_t;
typedef unsigned short uint_least16_t;
typedef int int_least32_t;
typedef unsigned int uint_least32_t;
typedef long int_least64_t;
typedef unsigned long uint_least64_t;
typedef signed char int_fast8_t;
typedef unsigned char uint_fast8_t;
typedef long int_fast16_t;
typedef unsigned long uint_fast16_t;
typedef long int_fast32_t;
typedef unsigned long uint_fast32_t;
typedef long int_fast64_t;
typedef unsigned long uint_fast64_t;

/* tcc does not implement GCC's overflow builtins. */
#define __builtin_add_overflow(a, b, res) ((*(res) = (a) + (b)), 0)
#define __builtin_sub_overflow(a, b, res) ((*(res) = (a) - (b)), 0)
#define __builtin_mul_overflow(a, b, res) ((*(res) = (a) * (b)), 0)

/* tcc cannot do 128-bit atomics; core's AtomicU128 paths are unreachable here. */
static int poc_cxchg128(unsigned __int128* obj, unsigned __int128* expected,
                        unsigned __int128 desired) {
    if (*obj == *expected) {
        *obj = desired;
        return 1;
    }
    *expected = *obj;
    return 0;
}
#endif
