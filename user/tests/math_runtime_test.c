#include <math.h>
#include <stdint.h>
#include <limits.h>

static int near(double value, double expected, double tolerance)
{
    return fabs(value - expected) <= tolerance;
}

int main(void)
{
    union {
        double value;
        uint64_t bits;
    } zero;

    int exponent = 0;
    double fraction;
    int quotient = 0;
    double nan_value = NAN;
    double huge_value = HUGE_VAL;
    union {
        float value;
        uint32_t bits;
    } zero_float;
    float float_fraction;
    int float_exponent = 0;

    if (nan_value == nan_value || !(huge_value > 1.0e300) ||
        !(INFINITY > 1.0e300))
        return 22;

    zero.value = copysign(0.0, -1.0);
    if (zero.bits != UINT64_C(0x8000000000000000))
        return 1;
    if (fabs(zero.value) != 0.0 || copysign(1.0, zero.value) != -1.0)
        return 2;

    if (trunc(-1.9) != -1.0 || floor(-1.2) != -2.0 || ceil(-1.2) != -1.0)
        return 3;
    if (round(-1.5) != -2.0 || nearbyint(2.5) != 2.0 ||
        nearbyint(3.5) != 4.0)
        return 4;
    zero.value = nearbyint(-0.5);
    if (zero.bits != UINT64_C(0x8000000000000000) || nan("test") == nan("test"))
        return 16;

    fraction = frexp(8.0, &exponent);
    if (fraction != 0.5 || exponent != 4 || ldexp(fraction, exponent) != 8.0)
        return 5;
    if (sqrt(9.0) != 3.0 || !near(cbrt(-27.0), -3.0, 1e-12))
        return 6;

    if (fmod(5.5, 2.0) != 1.5 || remainder(7.0, 2.0) != -1.0)
        return 7;
    if (remquo(7.0, 2.0, &quotient) != -1.0 || quotient != 4)
        return 8;
    if (remquo(-7.0, 2.0, &quotient) != 1.0 || quotient != -4)
        return 17;
    if (remquo(18.0, 2.0, &quotient) != 0.0 || quotient != 1)
        return 18;

    zero.value = nextafter(0.0, 1.0);
    if (zero.bits != 1)
        return 9;
    zero.value = nextafter(0.0, -1.0);
    if (zero.bits != UINT64_C(0x8000000000000001))
        return 38;
    zero.value = nextafter(INFINITY, 0.0);
    if (zero.bits != UINT64_C(0x7fefffffffffffff))
        return 39;
    zero.value = nextafter(-INFINITY, 0.0);
    if (zero.bits != UINT64_C(0xffefffffffffffff))
        return 40;
    zero.value = nextafter(0.0, -1.0);
    if (ilogb(8.0) != 3 || ilogb(zero.value) != -1074 ||
        ilogb(0.0) != FP_ILOGB0 || ilogb(NAN) != FP_ILOGBNAN ||
        ilogb(INFINITY) != INT_MAX)
        return 19;
    zero.value = logb(0.0);
    if (zero.bits != UINT64_C(0xfff0000000000000))
        return 21;
    if (!near(exp(1.0), 2.71828182845904523536, 1e-13) ||
        !near(log(2.71828182845904523536), 1.0, 1e-12))
        return 10;
    if (!near(pow(2.0, 10.0), 1024.0, 1e-12) ||
        !near(log2(8.0), 3.0, 1e-12))
        return 11;

    if (!near(sin(0.52359877559829887308), 0.5, 1e-12) ||
        !near(cos(1.04719755119659774615), 0.5, 1e-12) ||
        !near(atan(1.0), 0.78539816339744830962, 1e-12))
        return 12;
    if (!near(hypot(3.0, 4.0), 5.0, 1e-12) ||
        !near(atan2(0.0, -1.0), 3.14159265358979323846, 1e-12))
        return 13;
    if (atan2(-0.0, -1.0) != -3.14159265358979323846 ||
        pow(-2.0, 3.0) != -8.0)
        return 20;

    if (!near(tgamma(5.0), 24.0, 1e-8) || erf(0.0) != 0.0)
        return 14;

    if (!(acosh(1.0e200) > 460.0 && acosh(1.0e200) < 462.0) ||
        !(asinh(-1.0e200) < -460.0 && asinh(-1.0e200) > -462.0))
        return 27;

    zero.value = fmax(-0.0, 0.0);
    if (zero.bits != 0)
        return 15;
    zero.value = fmin(-0.0, 0.0);
    if (zero.bits != UINT64_C(0x8000000000000000))
        return 23;

    zero.value = fmax(-0.0, -0.0);
    if (zero.bits != UINT64_C(0x8000000000000000))
        return 28;
    zero.value = fmin(0.0, 0.0);
    if (zero.bits != 0)
        return 29;

    if (sqrt(-1.0) == sqrt(-1.0) || sqrt(-INFINITY) == sqrt(-INFINITY))
        return 24;
    if (!near(atan2(INFINITY, INFINITY), 0.78539816339744830962, 1e-12) ||
        !near(atan2(-INFINITY, -INFINITY), -2.35619449019234492885, 1e-12))
        return 25;
    if (remainder(-7.0, 2.0) != 1.0)
        return 26;

    if (fabsf(-2.0f) != 2.0f || sqrtf(81.0f) != 9.0f ||
        !near((double)sinf(0.5235987755982988f), 0.5, 1e-6))
        return 30;
    if (!near((double)expf(1.0f), 2.7182818, 1e-6) ||
        !near((double)logf(2.7182818f), 1.0, 1e-6))
        return 31;
    float_fraction = frexpf(8.0f, &float_exponent);
    if (float_fraction != 0.5f || float_exponent != 4 ||
        ldexpf(float_fraction, float_exponent) != 8.0f)
        return 32;
    if (modff(-3.25f, &float_fraction) != -0.25f ||
        float_fraction != -3.0f)
        return 33;
    zero_float.value = nextafterf(0.0f, 1.0f);
    if (zero_float.bits != 1U)
        return 34;
    zero_float.value = nextafterf(0.0f, -1.0f);
    if (zero_float.bits != UINT32_C(0x80000001))
        return 35;
    zero_float.value = nextafterf(INFINITY, 0.0f);
    if (zero_float.bits != UINT32_C(0x7f7fffff))
        return 41;
    zero_float.value = nextafterf(-INFINITY, 0.0f);
    if (zero_float.bits != UINT32_C(0xff7fffff))
        return 42;
    if (remquof(7.0f, 2.0f, &quotient) != -1.0f || quotient != 4)
        return 36;
    if (ilogbf(8.0f) != 3 || ilogbf(0.0f) != FP_ILOGB0 ||
        ilogbf(NAN) != FP_ILOGBNAN || ilogbf(INFINITY) != INT_MAX)
        return 45;
    zero_float.value = nextafterf(0.0f, -1.0f);
    if (ilogbf(zero_float.value) != -149)
        return 46;
    zero_float.value = fmaxf(-0.0f, -0.0f);
    if (zero_float.bits != UINT32_C(0x80000000))
        return 47;
    zero_float.value = fminf(0.0f, 0.0f);
    if (zero_float.bits != 0U)
        return 48;
    zero_float.value = nearbyintf(-0.5f);
    if (zero_float.bits != UINT32_C(0x80000000))
        return 49;
    if (nexttowardf(1.0f, 1.0L) != 1.0f || nanf("float") == nanf("float"))
        return 37;
    if (nexttoward(1.0, (long double)NAN) ==
        nexttoward(1.0, (long double)NAN))
        return 43;
    if (nexttowardf(1.0f, (long double)NAN) ==
        nexttowardf(1.0f, (long double)NAN))
        return 44;

    return 0;
}
