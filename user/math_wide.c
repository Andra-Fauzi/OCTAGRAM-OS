/*
 * C99 long double and scalbln variants of the double math functions.
 *
 * OCTAGRAM's libm is a bootstrap implementation written in double precision, so
 * these wrappers convert, call the double routine and convert back. They exist
 * only for the functions libm actually implements: a long double variant of an
 * unimplemented function would fail at link time, which is the honest signal
 * that libm still needs that routine.
 */
#include <math.h>
#include <stddef.h>

long double acoshl(long double arg0)
{
    return (long double)acosh((double)arg0);
}

long double acosl(long double arg0)
{
    return (long double)acos((double)arg0);
}

long double asinhl(long double arg0)
{
    return (long double)asinh((double)arg0);
}

long double asinl(long double arg0)
{
    return (long double)asin((double)arg0);
}

long double atan2l(long double arg0, long double arg1)
{
    return (long double)atan2((double)arg0, (double)arg1);
}

long double atanhl(long double arg0)
{
    return (long double)atanh((double)arg0);
}

long double atanl(long double arg0)
{
    return (long double)atan((double)arg0);
}

long double cbrtl(long double arg0)
{
    return (long double)cbrt((double)arg0);
}

long double ceill(long double arg0)
{
    return (long double)ceil((double)arg0);
}

long double copysignl(long double arg0, long double arg1)
{
    return (long double)copysign((double)arg0, (double)arg1);
}

long double coshl(long double arg0)
{
    return (long double)cosh((double)arg0);
}

long double cosl(long double arg0)
{
    return (long double)cos((double)arg0);
}

long double erfcl(long double arg0)
{
    return (long double)erfc((double)arg0);
}

long double erfl(long double arg0)
{
    return (long double)erf((double)arg0);
}

long double exp2l(long double arg0)
{
    return (long double)exp2((double)arg0);
}

long double expl(long double arg0)
{
    return (long double)exp((double)arg0);
}

long double expm1l(long double arg0)
{
    return (long double)expm1((double)arg0);
}

long double fabsl(long double arg0)
{
    return (long double)fabs((double)arg0);
}

long double fdiml(long double arg0, long double arg1)
{
    return (long double)fdim((double)arg0, (double)arg1);
}

long double floorl(long double arg0)
{
    return (long double)floor((double)arg0);
}

long double fmal(long double arg0, long double arg1, long double arg2)
{
    return (long double)fma((double)arg0, (double)arg1, (double)arg2);
}

long double fmaxl(long double arg0, long double arg1)
{
    return (long double)fmax((double)arg0, (double)arg1);
}

long double fminl(long double arg0, long double arg1)
{
    return (long double)fmin((double)arg0, (double)arg1);
}

long double fmodl(long double arg0, long double arg1)
{
    return (long double)fmod((double)arg0, (double)arg1);
}

long double frexpl(long double arg0, int * arg1)
{
    return (long double)frexp((double)arg0, arg1);
}

long double hypotl(long double arg0, long double arg1)
{
    return (long double)hypot((double)arg0, (double)arg1);
}

int ilogbl(long double arg0)
{
    return (int)ilogb((double)arg0);
}

long double lgammal(long double arg0)
{
    return (long double)lgamma((double)arg0);
}

long long llrintl(long double arg0)
{
    return (long long)llrint((double)arg0);
}

long long llroundl(long double arg0)
{
    return (long long)llround((double)arg0);
}

long double log10l(long double arg0)
{
    return (long double)log10((double)arg0);
}

long double log1pl(long double arg0)
{
    return (long double)log1p((double)arg0);
}

long double log2l(long double arg0)
{
    return (long double)log2((double)arg0);
}

long double logbl(long double arg0)
{
    return (long double)logb((double)arg0);
}

long double logl(long double arg0)
{
    return (long double)log((double)arg0);
}

long lrintl(long double arg0)
{
    return (long)lrint((double)arg0);
}

long lroundl(long double arg0)
{
    return (long)lround((double)arg0);
}

long double modfl(long double value, long double *integral)
{
    double part = 0.0;
    const double result = modf((double)value, &part);
    if (integral != NULL) {
        *integral = (long double)part;
    }
    return (long double)result;
}

long double nanl(const char *tag)
{
    (void)tag;
    return (long double)nan("");
}

long double nearbyintl(long double arg0)
{
    return (long double)nearbyint((double)arg0);
}

long double nextafterl(long double arg0, long double arg1)
{
    return (long double)nextafter((double)arg0, (double)arg1);
}

long double nexttowardl(long double arg0, long double arg1)
{
    return (long double)nexttoward((double)arg0, (double)arg1);
}

long double powl(long double arg0, long double arg1)
{
    return (long double)pow((double)arg0, (double)arg1);
}

long double remainderl(long double arg0, long double arg1)
{
    return (long double)remainder((double)arg0, (double)arg1);
}

long double remquol(long double arg0, long double arg1, int * arg2)
{
    return (long double)remquo((double)arg0, (double)arg1, arg2);
}

long double rintl(long double arg0)
{
    return (long double)rint((double)arg0);
}

long double roundl(long double arg0)
{
    return (long double)round((double)arg0);
}

static int oct_clamp_exponent(long exponent)
{
    if (exponent > 100000L) {
        return 100000;
    }
    if (exponent < -100000L) {
        return -100000;
    }
    return (int)exponent;
}

double scalbln(double value, long exponent)
{
    return ldexp(value, oct_clamp_exponent(exponent));
}

float scalblnf(float value, long exponent)
{
    return ldexpf(value, oct_clamp_exponent(exponent));
}

long double scalbnl(long double value, int exponent)
{
    return ldexpl(value, exponent);
}

long double scalblnl(long double value, long exponent)
{
    return ldexpl(value, oct_clamp_exponent(exponent));
}

long double sinhl(long double arg0)
{
    return (long double)sinh((double)arg0);
}

long double sinl(long double arg0)
{
    return (long double)sin((double)arg0);
}

long double sqrtl(long double arg0)
{
    return (long double)sqrt((double)arg0);
}

long double tanhl(long double arg0)
{
    return (long double)tanh((double)arg0);
}

long double tanl(long double arg0)
{
    return (long double)tan((double)arg0);
}

long double tgammal(long double arg0)
{
    return (long double)tgamma((double)arg0);
}

long double truncl(long double arg0)
{
    return (long double)trunc((double)arg0);
}
