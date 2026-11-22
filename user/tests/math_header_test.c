#include <math.h>

static double (*const p_fabs)(double) = fabs;
static double (*const p_sqrt)(double) = sqrt;
static double (*const p_floor)(double) = floor;
static double (*const p_ceil)(double) = ceil;
static double (*const p_trunc)(double) = trunc;
static double (*const p_round)(double) = round;
static double (*const p_pow)(double, double) = pow;
static double (*const p_copysign)(double, double) = copysign;
static double (*const p_fma)(double, double, double) = fma;
static double (*const p_exp)(double) = exp;
static double (*const p_log2)(double) = log2;
static double (*const p_cbrt)(double) = cbrt;
static double (*const p_fdim)(double, double) = fdim;
static double (*const p_nan)(const char *) = nan;
static float (*const p_sinf)(float) = sinf;
static float (*const p_expf)(float) = expf;
static float (*const p_logf)(float) = logf;
static float (*const p_nextafterf)(float, float) = nextafterf;
static float (*const p_nexttowardf)(float, long double) = nexttowardf;
static float (*const p_fmaf)(float, float, float) = fmaf;
static float (*const p_remquof)(float, float, int *) = remquof;
static volatile double huge_value = HUGE_VAL;
static volatile float huge_float_value = HUGE_VALF;
static volatile long double huge_long_double_value = HUGE_VALL;
static volatile float infinity_value = INFINITY;
static volatile float nan_value = NAN;

int main(void) {
    return !(p_fabs && p_sqrt && p_floor && p_ceil && p_trunc && p_round &&
             p_pow && p_copysign && p_fma && p_exp && p_log2 && p_cbrt &&
             p_fdim && p_nan && p_sinf && p_expf && p_logf && p_nextafterf &&
             p_nexttowardf && p_fmaf && p_remquof && huge_value > 1.0e300 &&
             huge_float_value > 1.0e30f &&
             huge_long_double_value > 1.0e300L &&
             infinity_value > 1.0e30f && nan_value != nan_value);
}
