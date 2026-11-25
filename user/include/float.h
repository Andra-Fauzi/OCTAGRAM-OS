/*
 * C floating-point characteristics for OCTAGRAM.
 *
 * x86_64 with SSE2 is the baseline: float is IEEE-754 binary32 and double is
 * binary64. `long double` follows the System V x86_64 ABI, i.e. the x87 80-bit
 * extended format (64-bit mantissa with an explicit integer bit), which is what
 * user/math.c, user/compiler_rt.c, and the long-double conversions assume.
 *
 * This header exists because libc++'s <cfloat> includes_next <float.h> and the
 * compiled library needs the macros: std::to_chars/std::from_chars for floating
 * point (src/charconv.cpp) is written in terms of FLT_MANT_DIG, FLT_MAX_EXP,
 * DBL_MANT_DIG, and friends.
 *
 * Values follow the C11/C17 requirements. the __STDC_VERSION__ >= 201112
 * macros (*_HAS_SUBNORM, *_DECIMAL_DIG) are only defined for C11 and later so a
 * C99 compiler does not see a typedef-free header change behaviour, and the
 * C23 names (*_NORM_MAX, *_INFINITY) are gated the same way.
 */
#ifndef _OCTAGRAM_FLOAT_H
#define _OCTAGRAM_FLOAT_H

/* Radix and evaluation method. SSE2 arithmetic keeps float/double precision,
 * and x87 is only used for `long double`, so nothing is evaluated in a wider
 * format than its declared type. */
#define FLT_RADIX 2
#define FLT_ROUNDS 1 /* round to nearest */
#define FLT_EVAL_METHOD 0

/* float: IEEE-754 binary32 */
#define FLT_MANT_DIG 24
#define FLT_DIG 6
#define FLT_DECIMAL_DIG 9
#define FLT_MIN_EXP (-125)
#define FLT_MIN_10_EXP (-37)
#define FLT_MAX_EXP 128
#define FLT_MAX_10_EXP 38
#define FLT_MAX 3.40282347e+38F
#define FLT_MIN 1.17549435e-38F
#define FLT_TRUE_MIN 1.40129846e-45F
#define FLT_EPSILON 1.19209290e-7F

/* double: IEEE-754 binary64 */
#define DBL_MANT_DIG 53
#define DBL_DIG 15
#define DBL_DECIMAL_DIG 17
#define DBL_MIN_EXP (-1021)
#define DBL_MIN_10_EXP (-307)
#define DBL_MAX_EXP 1024
#define DBL_MAX_10_EXP 308
#define DBL_MAX 1.7976931348623157e+308
#define DBL_MIN 2.2250738585072014e-308
#define DBL_TRUE_MIN 4.9406564584124654e-324
#define DBL_EPSILON 2.2204460492503131e-16

/* long double: x87 80-bit extended (System V x86_64) */
#define LDBL_MANT_DIG 64
#define LDBL_DIG 18
#define LDBL_DECIMAL_DIG 21
#define LDBL_MIN_EXP (-16381)
#define LDBL_MIN_10_EXP (-4931)
#define LDBL_MAX_EXP 16384
#define LDBL_MAX_10_EXP 4932
#define LDBL_MAX 1.18973149535723176502e+4932L
#define LDBL_MIN 3.36210314311209350626e-4932L
#define LDBL_TRUE_MIN 3.64519953188247460253e-4951L
#define LDBL_EPSILON 1.08420217248550443401e-19L

/* The widest type is long double. */
#define DECIMAL_DIG 21

#if !defined(__cplusplus) && defined(__STDC_VERSION__) && __STDC_VERSION__ >= 201112L
#define FLT_HAS_SUBNORM 1
#define DBL_HAS_SUBNORM 1
#define LDBL_HAS_SUBNORM 1
#endif

/* C23 spellings of the same limits. libc++ 18 already probes for these, and
 * GCC's freestanding <float.h> used to provide them, so misdetecting them
 * would change template selection. */
#if defined(__STDC_VERSION__) && __STDC_VERSION__ >= 202311L
#define FLT_NORM_MAX FLT_MAX
#define DBL_NORM_MAX DBL_MAX
#define LDBL_NORM_MAX LDBL_MAX
#define FLT_SNAN (__builtin_nansf(""))
#define DBL_SNAN (__builtin_nans(""))
#define LDBL_SNAN (__builtin_nansl(""))
#define FLT_INFINITY (__builtin_inff())
#define DBL_INFINITY (__builtin_inf())
#define LDBL_INFINITY (__builtin_infl())
#endif

#endif /* _OCTAGRAM_FLOAT_H */
