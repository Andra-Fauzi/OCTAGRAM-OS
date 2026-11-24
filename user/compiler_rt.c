/*
 * compiler-rt builtins that LLVM and gcc emit as libcalls.
 *
 * On x86-64 every 128-bit division and remainder becomes a call to
 * __udivmodti4/__divmodti4, so u128 division in C, C++ (`__int128`) and Rust
 * (`u128`) cannot link without them. The long division below deliberately only
 * uses 64-bit arithmetic and never `/` on a 128-bit value, which would recurse
 * back into these very functions.
 *
 * Only x86-64 little-endian is supported; that is the whole OCTAGRAM target set.
 */

typedef union {
    unsigned __int128 value;
    struct {
        unsigned long long low;
        unsigned long long high;
    } words;
} OctCompileRtU128;

typedef union {
    __int128 value;
    struct {
        unsigned long long low;
        unsigned long long high;
    } words;
} OctCompileRtI128;

static int oct_u128_greater_or_equal(OctCompileRtU128 left, OctCompileRtU128 right)
{
    if (left.words.high != right.words.high) {
        return left.words.high > right.words.high;
    }
    return left.words.low >= right.words.low;
}

static OctCompileRtU128 oct_u128_subtract(OctCompileRtU128 left, OctCompileRtU128 right)
{
    OctCompileRtU128 result;
    result.words.low = left.words.low - right.words.low;
    result.words.high = left.words.high - right.words.high
                        - (left.words.low < right.words.low ? 1ull : 0ull);
    return result;
}

static OctCompileRtU128 oct_u128_negate(OctCompileRtU128 value)
{
    OctCompileRtU128 zero;
    zero.value = 0;
    return oct_u128_subtract(zero, value);
}

static OctCompileRtU128 oct_u128_shift_left_one(OctCompileRtU128 value, unsigned long long bit,
                                                 int *carry_out)
{
    OctCompileRtU128 result;
    *carry_out = (int)(value.words.high >> 63);
    result.words.high = (value.words.high << 1) | (value.words.low >> 63);
    result.words.low = (value.words.low << 1) | (bit & 1ull);
    return result;
}

static int oct_u128_is_zero(OctCompileRtU128 value)
{
    return value.words.high == 0 && value.words.low == 0;
}

unsigned __int128 __udivmodti4(unsigned __int128 dividend, unsigned __int128 divisor,
                               unsigned __int128 *remainder);

unsigned __int128 __udivmodti4(unsigned __int128 dividend, unsigned __int128 divisor,
                               unsigned __int128 *remainder)
{
    OctCompileRtU128 top;
    OctCompileRtU128 bottom;
    top.value = dividend;
    bottom.value = divisor;
    if (oct_u128_is_zero(bottom)) {
        /* Match compiler-rt: division by zero is undefined behaviour. */
        __builtin_trap();
    }

    OctCompileRtU128 quotient;
    OctCompileRtU128 rest;
    quotient.value = 0;
    rest.value = 0;
    for (int bit = 127; bit >= 0; --bit) {
        unsigned long long incoming = (unsigned long long)((top.words.high >> 63) & 1ull);
        int carry = 0;
        rest = oct_u128_shift_left_one(rest, incoming, &carry);
        top.words.high = (top.words.high << 1) | (top.words.low >> 63);
        top.words.low <<= 1;
        /* A carry means the true remainder is 2^128 + rest, which is certainly
           >= divisor; subtracting modulo 2^128 still yields the right value
           because the result stays below the divisor. */
        if (carry || oct_u128_greater_or_equal(rest, bottom)) {
            rest = oct_u128_subtract(rest, bottom);
            if (bit >= 64) {
                quotient.words.high |= 1ull << (bit - 64);
            } else {
                quotient.words.low |= 1ull << bit;
            }
        }
    }
    if (remainder != 0) {
        *remainder = rest.value;
    }
    return quotient.value;
}

__int128 __divmodti4(__int128 dividend, __int128 divisor, __int128 *remainder);

__int128 __divmodti4(__int128 dividend, __int128 divisor, __int128 *remainder)
{
    OctCompileRtI128 top;
    OctCompileRtI128 bottom;
    top.value = dividend;
    bottom.value = divisor;
    const int top_negative = (int)(top.words.high >> 63);
    const int bottom_negative = (int)(bottom.words.high >> 63);
    OctCompileRtU128 top_magnitude;
    OctCompileRtU128 bottom_magnitude;
    top_magnitude.value = top.value;
    bottom_magnitude.value = bottom.value;
    if (top_negative) {
        top_magnitude = oct_u128_negate(top_magnitude);
    }
    if (bottom_negative) {
        bottom_magnitude = oct_u128_negate(bottom_magnitude);
    }
    if (oct_u128_is_zero(bottom_magnitude)) {
        __builtin_trap();
    }

    unsigned __int128 rest = 0;
    OctCompileRtU128 quotient;
    quotient.value = __udivmodti4(top_magnitude.value, bottom_magnitude.value, &rest);
    if (top_negative != bottom_negative) {
        quotient = oct_u128_negate(quotient);
    }
    OctCompileRtU128 rest_value;
    rest_value.value = rest;
    if (top_negative) {
        rest_value = oct_u128_negate(rest_value);
    }
    if (remainder != 0) {
        *remainder = rest_value.value;
    }
    /* The one overflow case, INT128_MIN / -1, wraps like two's complement. */
    return quotient.value;
}

/* Spelling variants other runtimes call directly. */
unsigned __int128 __udivti3(unsigned __int128 dividend, unsigned __int128 divisor)
{
    return __udivmodti4(dividend, divisor, 0);
}

unsigned __int128 __umodti3(unsigned __int128 dividend, unsigned __int128 divisor)
{
    unsigned __int128 remainder = 0;
    (void)__udivmodti4(dividend, divisor, &remainder);
    return remainder;
}

__int128 __divti3(__int128 dividend, __int128 divisor)
{
    return __divmodti4(dividend, divisor, 0);
}

__int128 __modti3(__int128 dividend, __int128 divisor)
{
    __int128 remainder = 0;
    (void)__divmodti4(dividend, divisor, &remainder);
    return remainder;
}

/* 128-bit variable shifts are libcalls on targets without a double-word shift. */
unsigned __int128 __ashlti3(unsigned __int128 value, int count);

unsigned __int128 __ashlti3(unsigned __int128 value, int count)
{
    OctCompileRtU128 input;
    input.value = value;
    if (count <= 0) {
        return value;
    }
    if (count >= 128) {
        return 0;
    }
    OctCompileRtU128 result;
    if (count >= 64) {
        result.words.high = input.words.low << (count - 64);
        result.words.low = 0;
    } else {
        result.words.high = (input.words.high << count) | (input.words.low >> (64 - count));
        result.words.low = input.words.low << count;
    }
    return result.value;
}

__int128 __ashrti3(__int128 value, int count);

__int128 __ashrti3(__int128 value, int count)
{
    OctCompileRtI128 input;
    input.value = value;
    if (count <= 0) {
        return value;
    }
    if (count >= 128) {
        return input.words.high >> 63 ? (__int128)-1 : (__int128)0;
    }
    OctCompileRtI128 result;
    const int negative = (int)(input.words.high >> 63);
    if (count >= 64) {
        result.words.high = negative ? ~0ull : 0ull;
        result.words.low = (unsigned long long)input.words.high >> (count - 64);
        if (negative && (count - 64) != 0) {
            result.words.low |= ~0ull << (64 - (count - 64));
        }
        return result.value;
    }
    result.words.low = (input.words.low >> count) | (input.words.high << (64 - count));
    result.words.high = negative
                            ? ((unsigned long long)input.words.high >> count) | (~0ull << (64 - count))
                            : (unsigned long long)input.words.high >> count;
    return result.value;
}

unsigned __int128 __lshrti3(unsigned __int128 value, int count);

unsigned __int128 __lshrti3(unsigned __int128 value, int count)
{
    OctCompileRtU128 input;
    input.value = value;
    if (count <= 0) {
        return value;
    }
    if (count >= 128) {
        return 0;
    }
    OctCompileRtU128 result;
    if (count >= 64) {
        result.words.low = input.words.high >> (count - 64);
        result.words.high = 0;
    } else {
        result.words.low = (input.words.low >> count) | (input.words.high << (64 - count));
        result.words.high = input.words.high >> count;
    }
    return result.value;
}

/*
 * Floating point conversions for 128-bit integers.
 *
 * gcc and LLVM emit these for `u128 as f64`, `f64 as u128` (Rust) and
 * `(double)__int128` (C/C++). They are implemented with integer bit assembly
 * rather than floating point arithmetic so the rounding is round-to-nearest,
 * ties-to-even, which is what the hardware and the C++ standard require.
 */

static int oct_u128_bit_length(OctCompileRtU128 value)
{
    if (value.words.high != 0) {
        return 64 + (64 - __builtin_clzll(value.words.high));
    }
    if (value.words.low != 0) {
        return 64 - __builtin_clzll(value.words.low);
    }
    return 0;
}

static OctCompileRtU128 oct_u128_shift_right(OctCompileRtU128 value, int count)
{
    OctCompileRtU128 result;
    if (count >= 128) {
        result.value = 0;
        return result;
    }
    if (count >= 64) {
        result.words.low = value.words.high >> (count - 64);
        result.words.high = 0;
        return result;
    }
    if (count == 0) {
        return value;
    }
    result.words.low = (value.words.low >> count) | (value.words.high << (64 - count));
    result.words.high = value.words.high >> count;
    return result;
}

static OctCompileRtU128 oct_u128_shift_left(OctCompileRtU128 value, int count)
{
    OctCompileRtU128 result;
    if (count >= 128) {
        result.value = 0;
        return result;
    }
    if (count >= 64) {
        result.words.high = value.words.low << (count - 64);
        result.words.low = 0;
        return result;
    }
    if (count == 0) {
        return value;
    }
    result.words.high = (value.words.high << count) | (value.words.low >> (64 - count));
    result.words.low = value.words.low << count;
    return result;
}

static int oct_u128_bit(OctCompileRtU128 value, int index)
{
    unsigned long long word =
        index >= 64 ? value.words.high : value.words.low;
    return (int)((word >> (index & 63)) & 1ull);
}

/* True when any bit below `count` is set. */
static int oct_u128_any_below(OctCompileRtU128 value, int count)
{
    if (count <= 0) {
        return 0;
    }
    if (count >= 128) {
        return !oct_u128_is_zero(value);
    }
    OctCompileRtU128 masked = oct_u128_shift_right(oct_u128_shift_left(value, 128 - count), 128 - count);
    return !oct_u128_is_zero(masked);
}

/* Reduce `value` (non-zero) to `keep_bits` significant bits, rounding halfway
 * cases to even, and report the exponent of its leading bit. */
static int oct_u128_normalise(OctCompileRtU128 value, int keep_bits, unsigned long long *mantissa)
{
    int length = oct_u128_bit_length(value);
    if (length <= keep_bits) {
        *mantissa = oct_u128_shift_left(value, keep_bits - length).words.low;
        return length - 1;
    }
    const int dropped = length - keep_bits;
    unsigned long long kept = oct_u128_shift_right(value, dropped).words.low;
    const int guard = oct_u128_bit(value, dropped - 1);
    const int sticky = oct_u128_any_below(value, dropped - 1);
    if (guard && (sticky || (kept & 1ull) != 0)) {
        ++kept;
        if (kept == (1ull << keep_bits)) {
            kept >>= 1;
            ++length;
        }
    }
    *mantissa = kept;
    return length - 1;
}

double __floatuntidf(unsigned __int128 value);

double __floatuntidf(unsigned __int128 value)
{
    OctCompileRtU128 input;
    input.value = value;
    if (oct_u128_is_zero(input)) {
        return 0.0;
    }
    unsigned long long mantissa = 0;
    const int exponent = oct_u128_normalise(input, 53, &mantissa);
    union {
        unsigned long long bits;
        double number;
    } result;
    result.bits = ((unsigned long long)(exponent + 1023) << 52) | (mantissa & ((1ull << 52) - 1));
    return result.number;
}

float __floatuntisf(unsigned __int128 value);

float __floatuntisf(unsigned __int128 value)
{
    OctCompileRtU128 input;
    input.value = value;
    if (oct_u128_is_zero(input)) {
        return 0.0f;
    }
    unsigned long long mantissa = 0;
    int exponent = oct_u128_normalise(input, 24, &mantissa);
    union {
        unsigned int bits;
        float number;
    } result;
    if (exponent + 127 >= 0xff) {
        result.bits = 0x7f800000u;
        return result.number;
    }
    result.bits = (unsigned int)(exponent + 127) << 23 | (unsigned int)(mantissa & ((1u << 23) - 1));
    return result.number;
}

double __floattidf(__int128 value);

double __floattidf(__int128 value)
{
    OctCompileRtI128 input;
    input.value = value;
    if ((input.words.high >> 63) == 0) {
        return __floatuntidf(value);
    }
    OctCompileRtU128 magnitude;
    magnitude.value = value;
    magnitude = oct_u128_negate(magnitude);
    return -__floatuntidf(magnitude.value);
}

float __floattisf(__int128 value);

float __floattisf(__int128 value)
{
    OctCompileRtI128 input;
    input.value = value;
    if ((input.words.high >> 63) == 0) {
        return __floatuntisf(value);
    }
    OctCompileRtU128 magnitude;
    magnitude.value = value;
    magnitude = oct_u128_negate(magnitude);
    return -__floatuntisf(magnitude.value);
}

unsigned __int128 __fixunsdfti(double value);

unsigned __int128 __fixunsdfti(double value)
{
    union {
        double number;
        unsigned long long bits;
    } input;
    input.number = value;
    const int negative = (int)(input.bits >> 63);
    const int exponent = (int)((input.bits >> 52) & 0x7ff);
    const unsigned long long fraction = input.bits & ((1ull << 52) - 1);
    if (exponent == 0x7ff) {
        /* NaN has no integer value; infinity saturates like compiler-rt. */
        return fraction != 0 ? (unsigned __int128)0 : ~(unsigned __int128)0;
    }
    if (negative || exponent == 0) {
        return 0;
    }
    OctCompileRtU128 mantissa;
    mantissa.words.high = 0;
    mantissa.words.low = fraction | (1ull << 52);
    const int shift = exponent - 1075;
    if (shift >= 0) {
        if (shift >= 128 || oct_u128_bit_length(mantissa) + shift > 128) {
            return ~(unsigned __int128)0;
        }
        return oct_u128_shift_left(mantissa, shift).value;
    }
    return oct_u128_shift_right(mantissa, -shift).value;
}

unsigned __int128 __fixunssfti(float value);

unsigned __int128 __fixunssfti(float value)
{
    return __fixunsdfti((double)value);
}

__int128 __fixdfti(double value);

__int128 __fixdfti(double value)
{
    union {
        double number;
        unsigned long long bits;
    } input;
    input.number = value;
    if ((input.bits >> 63) == 0) {
        return (__int128)__fixunsdfti(value);
    }
    OctCompileRtU128 magnitude;
    magnitude.value = __fixunsdfti(-value);
    return (__int128)oct_u128_negate(magnitude).value;
}

__int128 __fixsfti(float value);

__int128 __fixsfti(float value)
{
    return __fixdfti((double)value);
}
