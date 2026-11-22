/* Runtime hooks that mrustc's C backend expects the environment to provide. */
#include <setjmp.h>
#include <stddef.h>
#include <math.h>

__thread jmp_buf* mrustc_panic_target;
__thread void* mrustc_panic_value;

void _Unwind_Resume(void) {
    for (;;) {
    }
}

/* Math helpers: OCTAGRAM ships no libm, and this PoC program does not use
 * floating point, so these are deliberately simple (and imprecise) stubs that
 * only exist to satisfy the linker for unreachable core code paths. */
double fabs(double x) { return x < 0.0 ? -x : x; }
float fabsf(float x) { return x < 0.0f ? -x : x; }
double copysign(double x, double y) { return y < 0.0 ? -fabs(x) : fabs(x); }
float copysignf(float x, float y) { return y < 0.0f ? -fabsf(x) : fabsf(x); }
double trunc(double x) { return (double)(long long)x; }
double floor(double x) { double t = trunc(x); return (t > x) ? t - 1.0 : t; }
double ceil(double x) { double t = trunc(x); return (t < x) ? t + 1.0 : t; }
double round(double x) { return (x < 0.0) ? ceil(x - 0.5) : floor(x + 0.5); }
double sqrt(double x) {
    double guess = x > 1.0 ? x / 2.0 : 1.0;
    int i;
    if (x <= 0.0) {
        return 0.0;
    }
    for (i = 0; i < 40; i++) {
        guess = 0.5 * (guess + x / guess);
    }
    return guess;
}
float sqrtf(float x) { return (float)sqrt((double)x); }
double pow(double base, double exponent) {
    double result = 1.0;
    long long whole = (long long)exponent;
    long long i;
    for (i = 0; i < whole; i++) {
        result *= base;
    }
    return result;
}

/* Further libm stubs for unreachable core paths (see note above). */
double fma(double a, double b, double c) { return a * b + c; }
double fdim(double x, double y) { return x > y ? x - y : 0.0; }
double exp(double x) {
    double term = 1.0, sum = 1.0;
    int i;
    for (i = 1; i < 20; i++) {
        term *= x / (double)i;
        sum += term;
    }
    return sum;
}
double log2(double x) {
    double result = 0.0;
    if (x <= 0.0) {
        return -INFINITY;
    }
    while (x >= 2.0) { x /= 2.0; result += 1.0; }
    while (x < 1.0) { x *= 2.0; result -= 1.0; }
    return result;
}
double cbrt(double x) {
    double guess = x > 1.0 ? x / 3.0 : 1.0;
    int i;
    if (x <= 0.0) {
        return 0.0;
    }
    for (i = 0; i < 60; i++) {
        guess = (2.0 * guess + x / (guess * guess)) / 3.0;
    }
    return guess;
}
