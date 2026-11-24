#ifndef OCTAGRAM_MATH_H
#define OCTAGRAM_MATH_H

#ifdef __cplusplus
extern "C" {
#endif

/* Public declarations for OCTAGRAM's bootstrap math runtime.
 * Transcendental functions are currently approximate, not correctly-rounded. */
#ifndef __builtin_huge_val
#define __builtin_huge_val() (1.0 / 0.0)
#endif
#ifndef __builtin_huge_valf
#define __builtin_huge_valf() (1.0F / 0.0F)
#endif
#ifndef __builtin_huge_vall
#define __builtin_huge_vall() (1.0L / 0.0L)
#endif
#ifndef __builtin_nanf
#define __builtin_nanf(ignored) (0.0F / 0.0F)
#endif
/* C99 real-floating types. OCTAGRAM evaluates float expressions in double, so
 * float_t is double, matching FLT_EVAL_METHOD == 0 on this ABI. */
typedef double float_t;
typedef double double_t;

/* C99 classification macros, backed by compiler builtins so they stay
 * constant-foldable. */
#define FP_NAN 0
#define FP_INFINITE 1
#define FP_ZERO 2
#define FP_SUBNORMAL 3
#define FP_NORMAL 4

#define fpclassify(value) __builtin_fpclassify(FP_NAN, FP_INFINITE, FP_NORMAL, FP_SUBNORMAL, FP_ZERO, (value))
#define isfinite(value) __builtin_isfinite(value)
#define isinf(value) __builtin_isinf(value)
#define isnan(value) __builtin_isnan(value)
#define isnormal(value) __builtin_isnormal(value)
#define signbit(value) __builtin_signbit(value)
#define isgreater(left, right) __builtin_isgreater((left), (right))
#define isgreaterequal(left, right) __builtin_isgreaterequal((left), (right))
#define isless(left, right) __builtin_isless((left), (right))
#define islessequal(left, right) __builtin_islessequal((left), (right))
#define islessgreater(left, right) __builtin_islessgreater((left), (right))
#define isunordered(left, right) __builtin_isunordered((left), (right))

#ifndef FP_ILOGB0
#define FP_ILOGB0 (-2147483647 - 1)
#endif
#ifndef FP_ILOGBNAN
#define FP_ILOGBNAN (-2147483647 - 1)
#endif
#ifndef HUGE_VAL
#define HUGE_VAL (__builtin_huge_val())
#endif
#ifndef HUGE_VALF
#define HUGE_VALF (__builtin_huge_valf())
#endif
#ifndef HUGE_VALL
#define HUGE_VALL (__builtin_huge_vall())
#endif
#ifndef INFINITY
#define INFINITY (__builtin_huge_valf())
#endif
#ifndef NAN
#define NAN (__builtin_nanf(""))
#endif
double acos(double);
double asin(double);
double atan(double);
double atan2(double, double);
double cos(double);
double sin(double);
double tan(double);
double cosh(double);
double sinh(double);
double tanh(double);
double exp(double);
double frexp(double, int *);
double ldexp(double, int);
long double ldexpl(long double, int);
double log(double);
double log10(double);
double modf(double, double *);
double pow(double, double);
double sqrt(double);
double ceil(double);
double fabs(double);
double floor(double);
double fmod(double, double);

double acosh(double);
double asinh(double);
double atanh(double);
double exp2(double);
double expm1(double);
int ilogb(double);
double log1p(double);
double log2(double);
double logb(double);
double scalbn(double, int);
double cbrt(double);
double hypot(double, double);
double erf(double);
double erfc(double);
double tgamma(double);
double lgamma(double);
double nearbyint(double);
double rint(double);
long lrint(double);
long long llrint(double);
double round(double);
long lround(double);
long long llround(double);
double trunc(double);
double remainder(double, double);
double remquo(double, double, int *);
double copysign(double, double);
double nan(const char *);
double nextafter(double, double);
double nexttoward(double, long double);
double fdim(double, double);
double fmax(double, double);
double fmin(double, double);
double fma(double, double, double);

float acosf(float);
float asinf(float);
float atanf(float);
float atan2f(float, float);
float cosf(float);
float sinf(float);
float tanf(float);
float coshf(float);
float sinhf(float);
float tanhf(float);
float expf(float);
float frexpf(float, int *);
float ldexpf(float, int);
float logf(float);
float log10f(float);
float modff(float, float *);
float powf(float, float);
float sqrtf(float);
float ceilf(float);
float fabsf(float);
float floorf(float);
float fmodf(float, float);
float acoshf(float);
float asinhf(float);
float atanhf(float);
float exp2f(float);
float expm1f(float);
int ilogbf(float);
float log1pf(float);
float log2f(float);
float logbf(float);
float scalbnf(float, int);
float cbrtf(float);
float hypotf(float, float);
float erff(float);
float erfcf(float);
float tgammaf(float);
float lgammaf(float);
float nearbyintf(float);
float rintf(float);
long lrintf(float);
long long llrintf(float);
float roundf(float);
long lroundf(float);
long long llroundf(float);
float truncf(float);
float remainderf(float, float);
float remquof(float, float, int *);
float copysignf(float, float);
float nanf(const char *);
float nextafterf(float, float);
float nexttowardf(float, long double);
float fdimf(float, float);
float fmaxf(float, float);
float fminf(float, float);
float fmaf(float, float, float);






/* C99 long double and scalbln variants; see math_wide.c. */
long double acoshl(long double arg0);
long double acosl(long double arg0);
long double asinhl(long double arg0);
long double asinl(long double arg0);
long double atan2l(long double arg0, long double arg1);
long double atanhl(long double arg0);
long double atanl(long double arg0);
long double cbrtl(long double arg0);
long double ceill(long double arg0);
long double copysignl(long double arg0, long double arg1);
long double coshl(long double arg0);
long double cosl(long double arg0);
long double erfcl(long double arg0);
long double erfl(long double arg0);
long double exp2l(long double arg0);
long double expl(long double arg0);
long double expm1l(long double arg0);
long double fabsl(long double arg0);
long double fdiml(long double arg0, long double arg1);
long double floorl(long double arg0);
long double fmal(long double arg0, long double arg1, long double arg2);
long double fmaxl(long double arg0, long double arg1);
long double fminl(long double arg0, long double arg1);
long double fmodl(long double arg0, long double arg1);
long double frexpl(long double arg0, int * arg1);
long double hypotl(long double arg0, long double arg1);
int ilogbl(long double arg0);
long double lgammal(long double arg0);
long long llrintl(long double arg0);
long long llroundl(long double arg0);
long double log10l(long double arg0);
long double log1pl(long double arg0);
long double log2l(long double arg0);
long double logbl(long double arg0);
long double logl(long double arg0);
long lrintl(long double arg0);
long lroundl(long double arg0);
long double modfl(long double arg0, long double * arg1);
long double nanl(const char * arg0);
long double nearbyintl(long double arg0);
long double nextafterl(long double arg0, long double arg1);
long double nexttowardl(long double arg0, long double arg1);
long double powl(long double arg0, long double arg1);
long double remainderl(long double arg0, long double arg1);
long double remquol(long double arg0, long double arg1, int * arg2);
long double rintl(long double arg0);
long double roundl(long double arg0);
double scalbln(double arg0, long arg1);
float scalblnf(float arg0, long arg1);
long double scalbnl(long double arg0, int arg1);
long double sinhl(long double arg0);
long double sinl(long double arg0);
long double sqrtl(long double arg0);
long double tanhl(long double arg0);
long double tanl(long double arg0);
long double tgammal(long double arg0);
long double truncl(long double arg0);

long double scalblnl(long double arg0, long arg1);

#ifdef __cplusplus
}
#endif

#endif
