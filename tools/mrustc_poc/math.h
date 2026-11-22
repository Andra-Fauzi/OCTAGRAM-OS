/* Minimal math.h for mrustc-generated C on OCTAGRAM (no libm available). */
#ifndef OCTAGRAM_POC_MATH_H
#define OCTAGRAM_POC_MATH_H
#define INFINITY (1.0 / 0.0)
#define HUGE_VAL (1.0 / 0.0)
#define NAN (0.0 / 0.0)
double fabs(double);
float fabsf(float);
double sqrt(double);
float sqrtf(float);
double floor(double);
double ceil(double);
double trunc(double);
double round(double);
double pow(double, double);
double copysign(double, double);
float copysignf(float, float);
double fma(double, double, double);
double exp(double);
double log2(double);
double cbrt(double);
double fdim(double, double);
#endif
