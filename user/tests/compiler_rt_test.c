/*
 * Host test for the compiler-rt 128-bit helpers in user/compiler_rt.c.
 *
 * The expected values come from the host compiler's own `__int128` division,
 * so this checks the shift-subtract implementation against a real divider for
 * edge cases and a few hundred thousand pseudo-random pairs.
 */
#include <stdio.h>
#include <stdint.h>

/*
 * The host compiler lowers `__int128` division to the very libcalls under test,
 * so this build renames the implementation (see the Makefile rule). The
 * expected values below then still come from libgcc's original helpers instead
 * of from the code being checked.
 */
#define __udivmodti4 oct_udivmodti4
#define __divmodti4 oct_divmodti4
#define __udivti3 oct_udivti3
#define __umodti3 oct_umodti3
#define __divti3 oct_divti3
#define __modti3 oct_modti3
#define __ashlti3 oct_ashlti3
#define __ashrti3 oct_ashrti3
#define __lshrti3 oct_lshrti3
#define __floatuntidf oct_floatuntidf
#define __floattidf oct_floattidf
#define __floatuntisf oct_floatuntisf
#define __floattisf oct_floattisf
#define __fixunsdfti oct_fixunsdfti
#define __fixdfti oct_fixdfti
#define __fixunssfti oct_fixunssfti
#define __fixsfti oct_fixsfti

unsigned __int128 __udivmodti4(unsigned __int128 dividend, unsigned __int128 divisor,
                               unsigned __int128 *remainder);
__int128 __divmodti4(__int128 dividend, __int128 divisor, __int128 *remainder);
unsigned __int128 __udivti3(unsigned __int128 dividend, unsigned __int128 divisor);
unsigned __int128 __umodti3(unsigned __int128 dividend, unsigned __int128 divisor);
__int128 __divti3(__int128 dividend, __int128 divisor);
__int128 __modti3(__int128 dividend, __int128 divisor);
unsigned __int128 __ashlti3(unsigned __int128 value, int count);
__int128 __ashrti3(__int128 value, int count);
unsigned __int128 __lshrti3(unsigned __int128 value, int count);
double __floatuntidf(unsigned __int128 value);
double __floattidf(__int128 value);
float __floatuntisf(unsigned __int128 value);
float __floattisf(__int128 value);
unsigned __int128 __fixunsdfti(double value);
__int128 __fixdfti(double value);
unsigned __int128 __fixunssfti(float value);
__int128 __fixsfti(float value);

static int failures;

static uint64_t double_bits(double value)
{
    union {
        double number;
        uint64_t bits;
    } view;
    view.number = value;
    return view.bits;
}

static uint32_t float_bits(float value)
{
    union {
        float number;
        uint32_t bits;
    } view;
    view.number = value;
    return view.bits;
}

/* The expected values come from libgcc's own helpers, because this build
 * renamed the implementation under test. */
static void check_float_conversions(unsigned __int128 value)
{
    const __int128 signed_value = (__int128)value;
    if (double_bits(oct_floatuntidf(value)) != double_bits((double)value)) {
        ++failures;
        printf("FAIL __floatuntidf for %llx%016llx\n", (unsigned long long)(value >> 64),
               (unsigned long long)value);
    }
    if (float_bits(oct_floatuntisf(value)) != float_bits((float)value)) {
        ++failures;
        printf("FAIL __floatuntisf for %llx%016llx\n", (unsigned long long)(value >> 64),
               (unsigned long long)value);
    }
    if (double_bits(oct_floattidf(signed_value)) != double_bits((double)signed_value)) {
        ++failures;
        printf("FAIL __floattidf for %lld,%lld\n", (long long)(value >> 64), (long long)value);
    }
    if (float_bits(oct_floattisf(signed_value)) != float_bits((float)signed_value)) {
        ++failures;
        printf("FAIL __floattisf for %lld,%lld\n", (long long)(value >> 64), (long long)value);
    }

    /* And back: only in-range, non-negative doubles have a defined result. */
    const double as_double = (double)value;
    if (as_double >= 0.0 && as_double < 0x1p128) {
        if (oct_fixunsdfti(as_double) != (unsigned __int128)as_double) {
            ++failures;
            printf("FAIL __fixunsdfti for %.17g\n", as_double);
        }
    }
    const float as_float = (float)value;
    /* A float cannot reach 2^128, so 2^127 bounds the in-range region. */
    if (as_float >= 0.0f && as_float < 0x1p127f) {
        if (oct_fixunssfti(as_float) != (unsigned __int128)as_float) {
            ++failures;
            printf("FAIL __fixunssfti for %.9g\n", (double)as_float);
        }
    }
    if (as_double > 0.0 && as_double < 0x1p127) {
        if (oct_fixdfti(-as_double) != (__int128)(-as_double)) {
            ++failures;
            printf("FAIL __fixdfti for %.17g\n", -as_double);
        }
        if (oct_fixsfti(-as_float) != (__int128)(-as_float)) {
            ++failures;
            printf("FAIL __fixsfti for %.9g\n", -(double)as_float);
        }
    }
}

static void check_u128(const char *what, unsigned __int128 got, unsigned __int128 want,
                       unsigned __int128 left, unsigned __int128 right)
{
    if (got == want) {
        return;
    }
    ++failures;
    printf("FAIL %s: %llx%016llx / %llx%016llx -> %llx%016llx, want %llx%016llx\n", what,
           (unsigned long long)(left >> 64), (unsigned long long)left,
           (unsigned long long)(right >> 64), (unsigned long long)right,
           (unsigned long long)(got >> 64), (unsigned long long)got,
           (unsigned long long)(want >> 64), (unsigned long long)want);
}

static void check_i128(const char *what, __int128 got, __int128 want, __int128 left,
                       __int128 right)
{
    if (got == want) {
        return;
    }
    ++failures;
    printf("FAIL %s: %lld,%lld / %lld,%lld mismatch\n", what, (long long)(left >> 64),
           (long long)left, (long long)(right >> 64), (long long)right);
}

static uint64_t rng_state = 0x9e3779b97f4a7c15ull;

static uint64_t next_random(void)
{
    rng_state ^= rng_state << 13;
    rng_state ^= rng_state >> 7;
    rng_state ^= rng_state << 17;
    return rng_state;
}

static unsigned __int128 make_u128(void)
{
    return ((unsigned __int128)next_random() << 64) | next_random();
}

static void check_pair(unsigned __int128 left, unsigned __int128 right)
{
    unsigned __int128 expected_remainder = left % right;
    unsigned __int128 remainder = 0;
    unsigned __int128 quotient = __udivmodti4(left, right, &remainder);
    check_u128("udivmodti4 quotient", quotient, left / right, left, right);
    check_u128("udivmodti4 remainder", remainder, expected_remainder, left, right);
    check_u128("udivti3", __udivti3(left, right), left / right, left, right);
    check_u128("umodti3", __umodti3(left, right), expected_remainder, left, right);
}

static void check_signed_pair(__int128 left, __int128 right)
{
    /* Skip the only signed overflow, INT128_MIN / -1. */
    if (right == -1 && left == (__int128)((unsigned __int128)1 << 127)) {
        return;
    }
    __int128 remainder = 0;
    __int128 quotient = __divmodti4(left, right, &remainder);
    check_i128("divmodti4 quotient", quotient, left / right, left, right);
    check_i128("divmodti4 remainder", remainder, left % right, left, right);
    check_i128("divti3", __divti3(left, right), left / right, left, right);
    check_i128("modti3", __modti3(left, right), left % right, left, right);
}

int main(void)
{
    const unsigned __int128 one = 1;
    const unsigned __int128 max_u128 = ~(unsigned __int128)0;
    const unsigned __int128 high_bit = one << 127;
    const __int128 min_i128 = (__int128)(one << 127);
    const __int128 max_i128 = (__int128)((one << 127) - 1);

    const unsigned __int128 unsigned_edges[] = {1, 2, 3, 255, 256, 0xffffffffull,
                                                (unsigned __int128)0xffffffffffffffffull,
                                                one << 64, (one << 64) + 1, high_bit,
                                                high_bit - 1, max_u128, max_u128 - 1};
    const size_t edge_count = sizeof(unsigned_edges) / sizeof(unsigned_edges[0]);
    for (size_t left = 0; left < edge_count; ++left) {
        for (size_t right = 0; right < edge_count; ++right) {
            check_pair(unsigned_edges[left], unsigned_edges[right]);
            check_pair(unsigned_edges[right], unsigned_edges[left]);
            check_pair(max_u128, unsigned_edges[right]);
            check_pair(high_bit, unsigned_edges[right]);
        }
    }

    for (int index = 0; index < 200000; ++index) {
        unsigned __int128 left = make_u128();
        unsigned __int128 right = make_u128();
        if (right == 0) {
            right = 1;
        }
        /* Bias half the cases towards small divisors so the shifting path is
           exercised on values that actually fit. */
        if ((index & 1) != 0) {
            right = (right & 0xffffffffull) + 1;
        }
        check_pair(left, right);
        check_signed_pair((__int128)left, (__int128)right);
        check_float_conversions(left);
    }

    const __int128 signed_edges[] = {0, 1, -1, 2, -2, (__int128)0xffffffffffffffffull,
                                     -(__int128)0xffffffffffffffffull, min_i128, max_i128,
                                     min_i128 + 1, max_i128 / 3};
    const size_t signed_count = sizeof(signed_edges) / sizeof(signed_edges[0]);
    for (size_t left = 0; left < signed_count; ++left) {
        for (size_t right = 0; right < signed_count; ++right) {
            if (signed_edges[left] == 0 || signed_edges[right] == 0) {
                continue;
            }
            check_signed_pair(signed_edges[left], signed_edges[right]);
            check_signed_pair(signed_edges[right], signed_edges[left]);
        }
    }

    /* Shifts: compare against the host compiler's own 128-bit shifting. */
    for (int count = -2; count <= 130; ++count) {
        unsigned __int128 value = make_u128();
        __int128 signed_value = (__int128)value;
        unsigned __int128 expected_left =
            count <= 0 ? value : (count >= 128 ? 0 : (unsigned __int128)(value << count));
        if (__ashlti3(value, count) != expected_left) {
            ++failures;
            printf("FAIL ashlti3 at count %d\n", count);
        }
        unsigned __int128 expected_right =
            count <= 0 ? value : (count >= 128 ? 0 : (unsigned __int128)(value >> count));
        if (__lshrti3(value, count) != expected_right) {
            ++failures;
            printf("FAIL lshrti3 at count %d\n", count);
        }
        if (count > 0 && count < 128 && __ashrti3(signed_value, count) != (signed_value >> count)) {
            ++failures;
            printf("FAIL ashrti3 at count %d\n", count);
        }
    }

    if (failures != 0) {
        printf("compiler-rt 128-bit: FAIL (%d)\n", failures);
        return 1;
    }
    printf("compiler-rt 128-bit: PASS\n");
    return 0;
}
