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
double ilogb(double);
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

float fabsf(float);
float sqrtf(float);
float floorf(float);
float ceilf(float);
float truncf(float);
float roundf(float);
float copysignf(float, float);
float fmaf(float, float, float);

#ifdef __cplusplus
}
#endif

#endif
