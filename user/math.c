/*
 * Small freestanding IEEE-754 math library for OCTAGRAM-OS.
 *
 * This is a bootstrap libm, not a drop-in replacement for a fully tuned
 * correctly-rounded libm. Core IEEE bit operations are exact; transcendental
 * functions use range reduction and bounded polynomial/series approximations.
 */
#include <math.h>
#include <stdint.h>
#include <limits.h>

typedef union {
    double value;
    uint64_t bits;
} OctDoubleBits;

typedef union {
    float value;
    uint32_t bits;
} OctFloatBits;

#define OCT_D_SIGN UINT64_C(0x8000000000000000)
#define OCT_D_EXP  UINT64_C(0x7ff0000000000000)
#define OCT_D_FRAC UINT64_C(0x000fffffffffffff)
#define OCT_D_ONE  UINT64_C(0x3ff0000000000000)
#define OCT_D_INF  UINT64_C(0x7ff0000000000000)
#define OCT_D_QNAN UINT64_C(0x7ff8000000000000)

static const double oct_pi = 3.14159265358979323846264338327950288;
static const double oct_half_pi = 1.57079632679489661923132169163975144;
static const double oct_quarter_pi = 0.785398163397448309615660845819875721;
static const double oct_two_pi = 6.28318530717958647692528676655900577;
static const double oct_ln2 = 0.693147180559945309417232121458176568;
static const double oct_ln10 = 2.30258509299404568401799145468436421;
static const double oct_log2e = 1.44269504088896340735992468100189214;
static const double oct_sqrt_half = 0.707106781186547524400844362104849039;

static uint64_t oct_bits(double x)
{
    OctDoubleBits value;
    value.value = x;
    return value.bits;
}

static double oct_from_bits(uint64_t bits)
{
    OctDoubleBits value;
    value.bits = bits;
    return value.value;
}

static int oct_isnan(double x)
{
    uint64_t bits = oct_bits(x);
    return (bits & OCT_D_EXP) == OCT_D_EXP && (bits & OCT_D_FRAC) != 0;
}

static int oct_isinf(double x)
{
    return (oct_bits(x) & ~OCT_D_SIGN) == OCT_D_INF;
}

static int oct_signbit(double x)
{
    return (int)(oct_bits(x) >> 63);
}

static double oct_inf(int negative)
{
    return oct_from_bits(OCT_D_INF | (negative ? OCT_D_SIGN : 0));
}

static double oct_qnan(void)
{
    return oct_from_bits(OCT_D_QNAN);
}

static double oct_signed_zero(int negative)
{
    return oct_from_bits(negative ? OCT_D_SIGN : 0);
}

static int oct_is_integer(double x)
{
    return !oct_isnan(x) && !oct_isinf(x) && trunc(x) == x;
}

double fabs(double x)
{
    return oct_from_bits(oct_bits(x) & ~OCT_D_SIGN);
}

double copysign(double x, double y)
{
    return oct_from_bits((oct_bits(x) & ~OCT_D_SIGN) |
                         (oct_bits(y) & OCT_D_SIGN));
}

float fabsf(float x)
{
    OctFloatBits value;
    value.value = x;
    value.bits &= UINT32_C(0x7fffffff);
    return value.value;
}

float copysignf(float x, float y)
{
    OctFloatBits a, b;
    a.value = x;
    b.value = y;
    a.bits = (a.bits & UINT32_C(0x7fffffff)) |
             (b.bits & UINT32_C(0x80000000));
    return a.value;
}

double trunc(double x)
{
    uint64_t bits = oct_bits(x);
    unsigned int exponent = (unsigned int)((bits & OCT_D_EXP) >> 52);
    int unbiased = (int)exponent - 1023;
    uint64_t mask;

    if (exponent == 0x7ffu || unbiased >= 52)
        return x;
    if (unbiased < 0)
        return oct_signed_zero(oct_signbit(x));
    mask = (UINT64_C(1) << (52 - unbiased)) - 1;
    return oct_from_bits(bits & ~mask);
}

double floor(double x)
{
    double integer = trunc(x);
    if (oct_isnan(x) || integer == x)
        return x;
    return x < 0.0 ? integer - 1.0 : integer;
}

double ceil(double x)
{
    double integer = trunc(x);
    if (oct_isnan(x) || integer == x)
        return x;
    return x > 0.0 ? integer + 1.0 : integer;
}

double round(double x)
{
    double integer;
    double fraction;

    if (oct_isnan(x) || oct_isinf(x) || fabs(x) >= 4503599627370496.0)
        return x;
    integer = trunc(x);
    fraction = fabs(x - integer);
    if (fraction < 0.5)
        return integer;
    return integer + (oct_signbit(x) ? -1.0 : 1.0);
}

double nearbyint(double x)
{
    double lower;
    double fraction;
    long long integer;

    if (oct_isnan(x) || oct_isinf(x) || fabs(x) >= 4503599627370496.0)
        return x;
    lower = floor(x);
    fraction = x - lower;
    if (fraction < 0.5)
        return lower;
    if (fraction > 0.5)
        return lower + 1.0;
    integer = (long long)lower;
    return (integer & 1LL) ? lower + 1.0 : lower;
}

double rint(double x)
{
    /* Bootstrap implementation uses round-to-nearest-even. */
    return nearbyint(x);
}

long lrint(double x)
{
    return (long)rint(x);
}

long long llrint(double x)
{
    return (long long)rint(x);
}

long lround(double x)
{
    return (long)round(x);
}

long long llround(double x)
{
    return (long long)round(x);
}

double frexp(double x, int *exponent)
{
    uint64_t bits = oct_bits(x);
    unsigned int field = (unsigned int)((bits & OCT_D_EXP) >> 52);
    int adjustment = 0;

    if (exponent == 0)
        return x;
    if (field == 0x7ffu || (bits & ~OCT_D_SIGN) == 0) {
        *exponent = 0;
        return x;
    }
    if (field == 0) {
        x *= 18014398509481984.0; /* 2^54 */
        bits = oct_bits(x);
        field = (unsigned int)((bits & OCT_D_EXP) >> 52);
        adjustment = -54;
    }
    *exponent = (int)field - 1022 + adjustment;
    bits = (bits & ~OCT_D_EXP) | (UINT64_C(1022) << 52);
    return oct_from_bits(bits);
}

double ldexp(double x, int exponent)
{
    double scale;

    if (x == 0.0 || oct_isnan(x) || oct_isinf(x))
        return x;
    while (exponent > 1023) {
        x *= 8.9884656743115795386465259539451e307; /* 2^1023 */
        exponent -= 1023;
        if (oct_isinf(x))
            return x;
    }
    while (exponent < -1022) {
        x *= 2.2250738585072013830902327173324e-308; /* 2^-1022 */
        exponent += 1022;
        if (x == 0.0)
            return x;
    }
    scale = oct_from_bits((uint64_t)(exponent + 1023) << 52);
    return x * scale;
}

double scalbn(double x, int exponent)
{
    return ldexp(x, exponent);
}

int ilogb(double x)
{
    uint64_t bits = oct_bits(x) & ~OCT_D_SIGN;
    unsigned int field = (unsigned int)((bits & OCT_D_EXP) >> 52);
    uint64_t fraction = bits & OCT_D_FRAC;
    int top = -1;

    if (bits == 0)
        return INT_MIN;
    if (field == 0x7ffu)
        return fraction ? INT_MAX : INT_MAX;
    if (field != 0)
        return (int)field - 1023;
    while (fraction != 0) {
        fraction >>= 1;
        ++top;
    }
    return top - 1074;
}

double logb(double x)
{
    if (oct_isnan(x))
        return x;
    if (x == 0.0)
        return oct_inf(1);
    if (oct_isinf(x))
        return oct_inf(0);
    return (double)ilogb(x);
}

double modf(double x, double *integer_part)
{
    double integer;
    double fraction;

    if (integer_part == 0)
        return oct_qnan();
    if (oct_isnan(x)) {
        *integer_part = x;
        return x;
    }
    if (oct_isinf(x)) {
        *integer_part = x;
        return oct_signed_zero(oct_signbit(x));
    }
    integer = trunc(x);
    fraction = x - integer;
    if (fraction == 0.0)
        fraction = oct_signed_zero(oct_signbit(x));
    *integer_part = integer;
    return fraction;
}

double sqrt(double x)
{
    double result;

    if (oct_isnan(x) || x == 0.0 || oct_isinf(x))
        return x;
    if (x < 0.0)
        return oct_qnan();
    __asm__ __volatile__("fldl %1\n\tfsqrt\n\tfstpl %0"
                         : "=m"(result) : "m"(x));
    return result;
}

float sqrtf(float x)
{
    return (float)sqrt((double)x);
}

double fmod(double x, double y)
{
    double ax, ay;
    int ex, ey;

    if (oct_isnan(x) || oct_isnan(y) || y == 0.0 || oct_isinf(x))
        return oct_qnan();
    if (oct_isinf(y) || x == 0.0)
        return x;
    ax = fabs(x);
    ay = fabs(y);
    if (ax < ay)
        return x;

    (void)frexp(ax, &ex);
    (void)frexp(ay, &ey);
    while (ax >= ay) {
        int shift = ex - ey;
        double scaled = ldexp(ay, shift);
        if (scaled > ax) {
            --ex;
            continue;
        }
        ax -= scaled;
        if (ax == 0.0)
            return oct_signed_zero(oct_signbit(x));
        (void)frexp(ax, &ex);
    }
    return copysign(ax, x);
}

double remainder(double x, double y)
{
    double r, ay;

    r = fmod(x, y);
    if (oct_isnan(r) || r == 0.0)
        return r;
    ay = fabs(y);
    if (fabs(r) > ay * 0.5)
        r -= copysign(ay, r);
    return r;
}

double remquo(double x, double y, int *quotient)
{
    double ratio;
    int low = 0;

    if (quotient != 0) {
        if (y != 0.0 && !oct_isnan(x) && !oct_isnan(y) &&
            !oct_isinf(x) && !oct_isinf(y)) {
            ratio = trunc(x / y);
            if (fabs(ratio) < 4503599627370496.0)
                low = (int)((long long)ratio & 7LL);
        }
        *quotient = low;
    }
    return remainder(x, y);
}

double nextafter(double x, double y)
{
    uint64_t bits;

    if (oct_isnan(x) || oct_isnan(y))
        return x + y;
    if (x == y)
        return y;
    if (x == 0.0)
        return oct_from_bits((oct_signbit(y) ? OCT_D_SIGN : 0) | 1);
    bits = oct_bits(x);
    if ((x > 0.0) == (y > x))
        ++bits;
    else
        --bits;
    return oct_from_bits(bits);
}

double nexttoward(double x, long double y)
{
    if ((long double)x == y)
        return (double)y;
    return nextafter(x, y > (long double)x ? oct_inf(0) : oct_inf(1));
}

double fdim(double x, double y)
{
    if (oct_isnan(x) || oct_isnan(y))
        return x + y;
    return x > y ? x - y : 0.0;
}

double fmax(double x, double y)
{
    if (oct_isnan(x))
        return y;
    if (oct_isnan(y))
        return x;
    if (x == 0.0 && y == 0.0)
        return oct_signed_zero(0);
    return x > y ? x : y;
}

double fmin(double x, double y)
{
    if (oct_isnan(x))
        return y;
    if (oct_isnan(y))
        return x;
    if (x == 0.0 && y == 0.0)
        return oct_signed_zero(1);
    return x < y ? x : y;
}

double hypot(double x, double y)
{
    double ax = fabs(x), ay = fabs(y), scale, ratio;

    if (oct_isinf(x) || oct_isinf(y))
        return oct_inf(0);
    if (oct_isnan(x) || oct_isnan(y))
        return x + y;
    scale = ax > ay ? ax : ay;
    if (scale == 0.0)
        return 0.0;
    ratio = (ax > ay ? ay : ax) / scale;
    return scale * sqrt(1.0 + ratio * ratio);
}

double exp(double x)
{
    int n, i;
    double r, term, sum;

    if (oct_isnan(x))
        return x;
    if (x > 709.782712893383973096)
        return oct_inf(0);
    if (x < -745.13321910194110842)
        return 0.0;
    if (oct_isinf(x))
        return x > 0.0 ? x : 0.0;

    n = (int)nearbyint(x * oct_log2e);
    r = x - (double)n * oct_ln2;
    term = 1.0;
    sum = 1.0;
    for (i = 1; i <= 22; ++i) {
        term *= r / (double)i;
        sum += term;
    }
    return ldexp(sum, n);
}

double exp2(double x)
{
    return exp(x * oct_ln2);
}

double expm1(double x)
{
    double term, sum;
    int i;

    if (fabs(x) > 0.25)
        return exp(x) - 1.0;
    term = x;
    sum = x;
    for (i = 2; i <= 28; ++i) {
        term *= x / (double)i;
        sum += term;
    }
    return sum;
}

double log(double x)
{
    double m, z, z2, term, sum;
    int exponent, denominator;

    if (oct_isnan(x) || x < 0.0)
        return oct_qnan();
    if (x == 0.0)
        return oct_inf(1);
    if (oct_isinf(x))
        return x;

    m = frexp(x, &exponent);
    if (m < oct_sqrt_half) {
        m *= 2.0;
        --exponent;
    }
    z = (m - 1.0) / (m + 1.0);
    z2 = z * z;
    term = z;
    sum = z;
    for (denominator = 3; denominator <= 41; denominator += 2) {
        term *= z2;
        sum += term / (double)denominator;
    }
    return 2.0 * sum + (double)exponent * oct_ln2;
}

double log2(double x)
{
    return log(x) * oct_log2e;
}

double log10(double x)
{
    return log(x) / oct_ln10;
}

double log1p(double x)
{
    double term, sum;
    int i;

    if (x == -1.0)
        return oct_inf(1);
    if (x < -1.0 || oct_isnan(x))
        return oct_qnan();
    if (fabs(x) > 0.25)
        return log(1.0 + x);
    term = x;
    sum = x;
    for (i = 2; i <= 48; ++i) {
        term *= -x;
        sum += term / (double)i;
    }
    return sum;
}

double pow(double x, double y)
{
    double result = 1.0, base, exponent;
    int negative_result = 0;

    if (y == 0.0 || x == 1.0)
        return 1.0;
    if (oct_isnan(x) || oct_isnan(y))
        return oct_qnan();
    if (oct_isinf(y)) {
        double ax = fabs(x);
        if (ax == 1.0)
            return 1.0;
        return ((ax > 1.0) == (y > 0.0)) ? oct_inf(0) : 0.0;
    }
    if (x == 0.0) {
        if (y < 0.0)
            return oct_inf(oct_signbit(x) && oct_is_integer(y) &&
                           nearbyint(y * 0.5) != y * 0.5);
        return oct_signed_zero(oct_signbit(x) && oct_is_integer(y) &&
                               nearbyint(y * 0.5) != y * 0.5);
    }
    if (x < 0.0) {
        if (!oct_is_integer(y))
            return oct_qnan();
        negative_result = nearbyint(y * 0.5) != y * 0.5;
        x = -x;
    }

    if (oct_is_integer(y) && fabs(y) <= 2147483647.0) {
        exponent = fabs(y);
        base = x;
        while (exponent >= 1.0) {
            if (exponent - 2.0 * (double)(long long)(exponent * 0.5) >= 1.0)
                result *= base;
            base *= base;
            exponent = (double)(long long)(exponent * 0.5);
        }
        if (y < 0.0)
            result = 1.0 / result;
    } else {
        result = exp(y * log(x));
    }
    return negative_result ? -result : result;
}

double atan(double x)
{
    double ax, z, z2, term, sum, result;
    int i;

    if (oct_isnan(x))
        return x;
    if (oct_isinf(x))
        return copysign(oct_half_pi, x);
    ax = fabs(x);
    if (ax > 1.0)
        return copysign(oct_half_pi - atan(1.0 / ax), x);
    if (ax > 0.4142135623730950488) {
        z = (ax - 1.0) / (ax + 1.0);
        result = oct_quarter_pi;
    } else {
        z = ax;
        result = 0.0;
    }
    z2 = z * z;
    term = z;
    sum = z;
    for (i = 3; i <= 81; i += 2) {
        term *= -z2;
        sum += term / (double)i;
    }
    result += sum;
    return copysign(result, x);
}

double atan2(double y, double x)
{
    if (oct_isnan(x) || oct_isnan(y))
        return x + y;
    if (x > 0.0)
        return atan(y / x);
    if (x < 0.0)
        return y >= 0.0 ? atan(y / x) + oct_pi
                        : atan(y / x) - oct_pi;
    if (y > 0.0)
        return oct_half_pi;
    if (y < 0.0)
        return -oct_half_pi;
    return copysign(0.0, y);
}

double asin(double x)
{
    if (oct_isnan(x) || fabs(x) > 1.0)
        return oct_qnan();
    if (x == 1.0)
        return oct_half_pi;
    if (x == -1.0)
        return -oct_half_pi;
    return atan2(x, sqrt((1.0 - x) * (1.0 + x)));
}

double acos(double x)
{
    if (oct_isnan(x) || fabs(x) > 1.0)
        return oct_qnan();
    return oct_half_pi - asin(x);
}

double sin(double x)
{
    double term, sum, x2;
    int i;

    if (oct_isnan(x) || oct_isinf(x))
        return oct_qnan();
    x = fmod(x, oct_two_pi);
    if (x > oct_pi)
        x -= oct_two_pi;
    else if (x < -oct_pi)
        x += oct_two_pi;
    x2 = x * x;
    term = x;
    sum = x;
    for (i = 1; i <= 14; ++i) {
        term *= -x2 / ((double)(2 * i) * (double)(2 * i + 1));
        sum += term;
    }
    return sum;
}

double cos(double x)
{
    double term, sum, x2;
    int i;

    if (oct_isnan(x) || oct_isinf(x))
        return oct_qnan();
    x = fmod(x, oct_two_pi);
    if (x > oct_pi)
        x -= oct_two_pi;
    else if (x < -oct_pi)
        x += oct_two_pi;
    x2 = x * x;
    term = 1.0;
    sum = 1.0;
    for (i = 1; i <= 14; ++i) {
        term *= -x2 / ((double)(2 * i - 1) * (double)(2 * i));
        sum += term;
    }
    return sum;
}

double tan(double x)
{
    double cosine = cos(x);
    if (cosine == 0.0)
        return copysign(oct_inf(0), sin(x));
    return sin(x) / cosine;
}

double sinh(double x)
{
    double positive, negative;
    if (oct_isnan(x) || oct_isinf(x))
        return x;
    positive = exp(x);
    negative = exp(-x);
    return (positive - negative) * 0.5;
}

double cosh(double x)
{
    double positive, negative;
    if (oct_isnan(x))
        return x;
    if (oct_isinf(x))
        return oct_inf(0);
    positive = exp(x);
    negative = exp(-x);
    return (positive + negative) * 0.5;
}

double tanh(double x)
{
    double e;
    if (oct_isnan(x))
        return x;
    if (x > 20.0)
        return 1.0;
    if (x < -20.0)
        return -1.0;
    e = expm1(2.0 * fabs(x));
    return copysign(e / (e + 2.0), x);
}

double acosh(double x)
{
    if (oct_isnan(x) || x < 1.0)
        return oct_qnan();
    if (x == 1.0)
        return 0.0;
    return log(x + sqrt(x - 1.0) * sqrt(x + 1.0));
}

double asinh(double x)
{
    double ax;
    if (oct_isnan(x) || oct_isinf(x) || x == 0.0)
        return x;
    ax = fabs(x);
    return copysign(log(ax + sqrt(ax * ax + 1.0)), x);
}

double atanh(double x)
{
    if (oct_isnan(x) || fabs(x) > 1.0)
        return oct_qnan();
    if (fabs(x) == 1.0)
        return copysign(oct_inf(0), x);
    return 0.5 * log1p((2.0 * x) / (1.0 - x));
}

double cbrt(double x)
{
    double mantissa, guess;
    int exponent, scale, i;

    if (oct_isnan(x) || oct_isinf(x) || x == 0.0)
        return x;
    mantissa = frexp(fabs(x), &exponent);
    scale = exponent / 3;
    if (exponent < 0 && exponent % 3)
        --scale;
    mantissa = ldexp(mantissa, exponent - 3 * scale);
    guess = 0.7;
    for (i = 0; i < 12; ++i)
        guess = (2.0 * guess + mantissa / (guess * guess)) / 3.0;
    return copysign(ldexp(guess, scale), x);
}

double erf(double x)
{
    /* Abramowitz-Stegun 7.1.26; maximum absolute error is about 1.5e-7. */
    const double p = 0.3275911;
    const double a1 = 0.254829592;
    const double a2 = -0.284496736;
    const double a3 = 1.421413741;
    const double a4 = -1.453152027;
    const double a5 = 1.061405429;
    double t, polynomial, value;

    if (oct_isnan(x))
        return x;
    if (oct_isinf(x))
        return copysign(1.0, x);
    t = 1.0 / (1.0 + p * fabs(x));
    polynomial = (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t;
    value = 1.0 - polynomial * exp(-x * x);
    return copysign(value, x);
}

double erfc(double x)
{
    return 1.0 - erf(x);
}

static double oct_sin_pi(double x)
{
    double reduced = fmod(x, 2.0);
    if (reduced > 1.0)
        reduced -= 2.0;
    if (reduced < -1.0)
        reduced += 2.0;
    return sin(reduced * oct_pi);
}

double lgamma(double x)
{
    static const double coefficients[9] = {
        0.99999999999980993227684700473478,
        676.520368121885098567009190444019,
        -1259.13921672240287047156078755283,
        771.3234287776530788486528258894,
        -176.61502916214059906584551354,
        12.507343278686904814458936853,
        -0.13857109526572011689554707,
        0.000009984369578019570859563,
        0.000000150563273514931155834
    };
    double z, sum;
    int i;

    if (oct_isnan(x))
        return x;
    if (x == 0.0 || (x < 0.0 && oct_is_integer(x)))
        return oct_inf(0);
    if (x < 0.5)
        return log(oct_pi) - log(fabs(oct_sin_pi(x))) - lgamma(1.0 - x);
    z = x - 1.0;
    sum = coefficients[0];
    for (i = 1; i < 9; ++i)
        sum += coefficients[i] / (z + (double)i);
    {
        double t = z + 7.5;
        return 0.91893853320467274178032973640562 +
               (z + 0.5) * log(t) - t + log(sum);
    }
}

double tgamma(double x)
{
    double magnitude, sign = 1.0;
    if (oct_isnan(x))
        return x;
    if (x == 0.0 || (x < 0.0 && oct_is_integer(x)))
        return copysign(oct_inf(0), x);
    if (x < 0.0 && oct_sin_pi(x) < 0.0)
        sign = -1.0;
    magnitude = exp(lgamma(x));
    return sign * magnitude;
}

double fma(double x, double y, double z)
{
    /* Extended precision reduces, but does not eliminate, double rounding. */
    return (double)((long double)x * (long double)y + (long double)z);
}

float floorf(float x)
{
    return (float)floor((double)x);
}

float ceilf(float x)
{
    return (float)ceil((double)x);
}

float truncf(float x)
{
    return (float)trunc((double)x);
}

float roundf(float x)
{
    return (float)round((double)x);
}

float fmaf(float x, float y, float z)
{
    return (float)fma((double)x, (double)y, (double)z);
}

