/*
 * Runtime support for TinyCC's type-generic integer overflow builtins.
 * The compiler widens operands to unsigned long long and passes their original
 * signedness, plus the pointed-to result width and signedness.
 */
typedef unsigned long long oct_u64;
typedef signed long long oct_i64;
typedef unsigned __int128 oct_u128;
typedef __int128 oct_i128;

static oct_i128 decode_operand(oct_u64 bits, int is_unsigned)
{
    union {
        oct_u64 u;
        oct_i64 s;
    } value;

    if (is_unsigned)
        return (oct_i128)bits;
    value.u = bits;
    return (oct_i128)value.s;
}

static void store_low_bytes(volatile void *destination, int size,
                            oct_u64 bits)
{
    volatile unsigned char *out = (volatile unsigned char *)destination;
    int i;

    /* OCTAGRAM's supported target is little-endian x86-64. Character stores
       may alias every integer object type and preserve volatile writes. */
    for (i = 0; i < size; ++i)
        out[i] = (unsigned char)(bits >> (i * 8));
}

/*
 * op: 0 = add, 1 = subtract, 2 = multiply
 * Return nonzero exactly when the mathematical result is not representable
 * in the destination integer type. The low result bits are always stored.
 */
int __tcc_overflow(oct_u64 a_bits, oct_u64 b_bits, volatile void *result,
                   int op, int a_unsigned, int b_unsigned,
                   int result_size, int result_unsigned)
{
    oct_i128 a = decode_operand(a_bits, a_unsigned);
    oct_i128 b = decode_operand(b_bits, b_unsigned);
    oct_u128 unsigned_max;
    oct_i128 signed_max, signed_min;
    oct_i128 value = 0;
    oct_u64 low_bits = 0;
    int overflow = 0;

    if (result_size != 1 && result_size != 2
        && result_size != 4 && result_size != 8)
        return 1;

    unsigned_max = ((oct_u128)1 << (result_size * 8)) - 1;
    signed_max = ((oct_i128)1 << (result_size * 8 - 1)) - 1;
    signed_min = -signed_max - 1;

    switch (op) {
    case 0:
        value = a + b;
        low_bits = (oct_u64)(oct_u128)value;
        break;
    case 1:
        value = a - b;
        low_bits = (oct_u64)(oct_u128)value;
        break;
    case 2: {
        int negative = ((a < 0) != (b < 0));
        oct_u128 abs_a = a < 0 ? (oct_u128)(-a) : (oct_u128)a;
        oct_u128 abs_b = b < 0 ? (oct_u128)(-b) : (oct_u128)b;
        oct_u128 product = abs_a * abs_b;

        if (result_unsigned) {
            overflow = (negative && product != 0) || product > unsigned_max;
        } else {
            oct_u128 limit = negative
                ? (oct_u128)(signed_max + 1)
                : (oct_u128)signed_max;
            overflow = product > limit;
        }
        low_bits = (oct_u64)product;
        if (negative)
            low_bits = (oct_u64)(0ULL - low_bits);
        store_low_bytes(result, result_size, low_bits);
        return overflow;
    }
    default:
        return 1;
    }

    if (result_unsigned) {
        overflow = value < 0 || (oct_u128)value > unsigned_max;
    } else {
        overflow = value < signed_min || value > signed_max;
    }
    store_low_bytes(result, result_size, low_bits);
    return overflow;
}
