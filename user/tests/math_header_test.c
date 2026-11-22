#include <math.h>
#include <stdint.h>

static double (*const p_acos)(double) = acos;
static double (*const p_asin)(double) = asin;
static double (*const p_atan)(double) = atan;
static double (*const p_atan2)(double, double) = atan2;
static double (*const p_cos)(double) = cos;
static double (*const p_sin)(double) = sin;
static double (*const p_tan)(double) = tan;
static double (*const p_cosh)(double) = cosh;
static double (*const p_sinh)(double) = sinh;
static double (*const p_tanh)(double) = tanh;
static double (*const p_exp)(double) = exp;
static double (*const p_frexp)(double, int *) = frexp;
static double (*const p_ldexp)(double, int) = ldexp;
static double (*const p_log)(double) = log;
static double (*const p_log10)(double) = log10;
static double (*const p_modf)(double, double *) = modf;
static double (*const p_pow)(double, double) = pow;
static double (*const p_sqrt)(double) = sqrt;
static double (*const p_ceil)(double) = ceil;
static double (*const p_fabs)(double) = fabs;
static double (*const p_floor)(double) = floor;
static double (*const p_fmod)(double, double) = fmod;
static double (*const p_acosh)(double) = acosh;
static double (*const p_asinh)(double) = asinh;
static double (*const p_atanh)(double) = atanh;
static double (*const p_exp2)(double) = exp2;
static double (*const p_expm1)(double) = expm1;
static int (*const p_ilogb)(double) = ilogb;
static double (*const p_log1p)(double) = log1p;
static double (*const p_log2)(double) = log2;
static double (*const p_logb)(double) = logb;
static double (*const p_scalbn)(double, int) = scalbn;
static double (*const p_cbrt)(double) = cbrt;
static double (*const p_hypot)(double, double) = hypot;
static double (*const p_erf)(double) = erf;
static double (*const p_erfc)(double) = erfc;
static double (*const p_tgamma)(double) = tgamma;
static double (*const p_lgamma)(double) = lgamma;
static double (*const p_nearbyint)(double) = nearbyint;
static double (*const p_rint)(double) = rint;
static long (*const p_lrint)(double) = lrint;
static long long (*const p_llrint)(double) = llrint;
static double (*const p_round)(double) = round;
static long (*const p_lround)(double) = lround;
static long long (*const p_llround)(double) = llround;
static double (*const p_trunc)(double) = trunc;
static double (*const p_remainder)(double, double) = remainder;
static double (*const p_remquo)(double, double, int *) = remquo;
static double (*const p_copysign)(double, double) = copysign;
static double (*const p_nan)(const char *) = nan;
static double (*const p_nextafter)(double, double) = nextafter;
static double (*const p_nexttoward)(double, long double) = nexttoward;
static double (*const p_fdim)(double, double) = fdim;
static double (*const p_fmax)(double, double) = fmax;
static double (*const p_fmin)(double, double) = fmin;
static double (*const p_fma)(double, double, double) = fma;
static float (*const p_acosf)(float) = acosf;
static float (*const p_asinf)(float) = asinf;
static float (*const p_atanf)(float) = atanf;
static float (*const p_atan2f)(float, float) = atan2f;
static float (*const p_cosf)(float) = cosf;
static float (*const p_sinf)(float) = sinf;
static float (*const p_tanf)(float) = tanf;
static float (*const p_coshf)(float) = coshf;
static float (*const p_sinhf)(float) = sinhf;
static float (*const p_tanhf)(float) = tanhf;
static float (*const p_expf)(float) = expf;
static float (*const p_frexpf)(float, int *) = frexpf;
static float (*const p_ldexpf)(float, int) = ldexpf;
static float (*const p_logf)(float) = logf;
static float (*const p_log10f)(float) = log10f;
static float (*const p_modff)(float, float *) = modff;
static float (*const p_powf)(float, float) = powf;
static float (*const p_sqrtf)(float) = sqrtf;
static float (*const p_ceilf)(float) = ceilf;
static float (*const p_fabsf)(float) = fabsf;
static float (*const p_floorf)(float) = floorf;
static float (*const p_fmodf)(float, float) = fmodf;
static float (*const p_acoshf)(float) = acoshf;
static float (*const p_asinhf)(float) = asinhf;
static float (*const p_atanhf)(float) = atanhf;
static float (*const p_exp2f)(float) = exp2f;
static float (*const p_expm1f)(float) = expm1f;
static int (*const p_ilogbf)(float) = ilogbf;
static float (*const p_log1pf)(float) = log1pf;
static float (*const p_log2f)(float) = log2f;
static float (*const p_logbf)(float) = logbf;
static float (*const p_scalbnf)(float, int) = scalbnf;
static float (*const p_cbrtf)(float) = cbrtf;
static float (*const p_hypotf)(float, float) = hypotf;
static float (*const p_erff)(float) = erff;
static float (*const p_erfcf)(float) = erfcf;
static float (*const p_tgammaf)(float) = tgammaf;
static float (*const p_lgammaf)(float) = lgammaf;
static float (*const p_nearbyintf)(float) = nearbyintf;
static float (*const p_rintf)(float) = rintf;
static long (*const p_lrintf)(float) = lrintf;
static long long (*const p_llrintf)(float) = llrintf;
static float (*const p_roundf)(float) = roundf;
static long (*const p_lroundf)(float) = lroundf;
static long long (*const p_llroundf)(float) = llroundf;
static float (*const p_truncf)(float) = truncf;
static float (*const p_remainderf)(float, float) = remainderf;
static float (*const p_remquof)(float, float, int *) = remquof;
static float (*const p_copysignf)(float, float) = copysignf;
static float (*const p_nanf)(const char *) = nanf;
static float (*const p_nextafterf)(float, float) = nextafterf;
static float (*const p_nexttowardf)(float, long double) = nexttowardf;
static float (*const p_fdimf)(float, float) = fdimf;
static float (*const p_fmaxf)(float, float) = fmaxf;
static float (*const p_fminf)(float, float) = fminf;
static float (*const p_fmaf)(float, float, float) = fmaf;

static volatile double huge_value = HUGE_VAL;
static volatile float huge_float_value = HUGE_VALF;
static volatile long double huge_long_double_value = HUGE_VALL;
static volatile float infinity_value = INFINITY;
static volatile float nan_value = NAN;

int main(void)
{
    return !(p_acos &&
             p_asin &&
             p_atan &&
             p_atan2 &&
             p_cos &&
             p_sin &&
             p_tan &&
             p_cosh &&
             p_sinh &&
             p_tanh &&
             p_exp &&
             p_frexp &&
             p_ldexp &&
             p_log &&
             p_log10 &&
             p_modf &&
             p_pow &&
             p_sqrt &&
             p_ceil &&
             p_fabs &&
             p_floor &&
             p_fmod &&
             p_acosh &&
             p_asinh &&
             p_atanh &&
             p_exp2 &&
             p_expm1 &&
             p_ilogb &&
             p_log1p &&
             p_log2 &&
             p_logb &&
             p_scalbn &&
             p_cbrt &&
             p_hypot &&
             p_erf &&
             p_erfc &&
             p_tgamma &&
             p_lgamma &&
             p_nearbyint &&
             p_rint &&
             p_lrint &&
             p_llrint &&
             p_round &&
             p_lround &&
             p_llround &&
             p_trunc &&
             p_remainder &&
             p_remquo &&
             p_copysign &&
             p_nan &&
             p_nextafter &&
             p_nexttoward &&
             p_fdim &&
             p_fmax &&
             p_fmin &&
             p_fma &&
             p_acosf &&
             p_asinf &&
             p_atanf &&
             p_atan2f &&
             p_cosf &&
             p_sinf &&
             p_tanf &&
             p_coshf &&
             p_sinhf &&
             p_tanhf &&
             p_expf &&
             p_frexpf &&
             p_ldexpf &&
             p_logf &&
             p_log10f &&
             p_modff &&
             p_powf &&
             p_sqrtf &&
             p_ceilf &&
             p_fabsf &&
             p_floorf &&
             p_fmodf &&
             p_acoshf &&
             p_asinhf &&
             p_atanhf &&
             p_exp2f &&
             p_expm1f &&
             p_ilogbf &&
             p_log1pf &&
             p_log2f &&
             p_logbf &&
             p_scalbnf &&
             p_cbrtf &&
             p_hypotf &&
             p_erff &&
             p_erfcf &&
             p_tgammaf &&
             p_lgammaf &&
             p_nearbyintf &&
             p_rintf &&
             p_lrintf &&
             p_llrintf &&
             p_roundf &&
             p_lroundf &&
             p_llroundf &&
             p_truncf &&
             p_remainderf &&
             p_remquof &&
             p_copysignf &&
             p_nanf &&
             p_nextafterf &&
             p_nexttowardf &&
             p_fdimf &&
             p_fmaxf &&
             p_fminf &&
             p_fmaf &&
             huge_value > 1.0e300 &&
             huge_float_value > 1.0e30f &&
             huge_long_double_value > 1.0e300L &&
             infinity_value > 1.0e30f &&
             nan_value != nan_value);
}
