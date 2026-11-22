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

int main(void) {
    return !(p_fabs && p_sqrt && p_floor && p_ceil && p_trunc && p_round &&
             p_pow && p_copysign && p_fma && p_exp && p_log2 && p_cbrt &&
             p_fdim);
}
