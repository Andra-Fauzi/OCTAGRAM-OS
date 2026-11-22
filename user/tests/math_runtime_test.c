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
    if (ilogb(8.0) != 3 || ilogb(zero.value) != -1074 ||
        ilogb(0.0) != INT_MIN || logb(0.0) != -1.0 / 0.0)
        return 19;
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
    if (!near(fmax(-0.0, 0.0), 0.0, 0.0) ||
        !near(fmin(-0.0, 0.0), -0.0, 0.0))
        return 15;

    return 0;
}
