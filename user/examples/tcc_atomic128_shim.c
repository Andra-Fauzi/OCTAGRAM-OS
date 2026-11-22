extern long write(int, const void *, unsigned long);

typedef struct {
    unsigned char bytes[16];
} octagram_u128_blob;

typedef struct {
    octagram_u128_blob _0;
} octagram_expected_holder;

/* The expression shape used by patch_core.py for AtomicU128. */
static int rewritten_atomic128_call(void *arg0,
                                    octagram_expected_holder var1,
                                    octagram_u128_blob arg2)
{
    return poc_cxchg128((void *)arg0, (void *)&var1._0,
                        (const void *)&arg2);
}

int main(void)
{
    octagram_u128_blob object;
    octagram_u128_blob desired;
    octagram_expected_holder expected;
    unsigned int index;
    static const char pass[] = "TCC-ATOMIC128-SHIM-PASS\n";

    for (index = 0; index < 16; ++index) {
        object.bytes[index] = (unsigned char)index;
        expected._0.bytes[index] = (unsigned char)index;
        desired.bytes[index] = (unsigned char)(0xf0u - index);
    }

    if (!rewritten_atomic128_call(&object, expected, desired))
        return 1;
    for (index = 0; index < 16; ++index) {
        if (object.bytes[index] != desired.bytes[index])
            return 2;
    }

    for (index = 0; index < 16; ++index)
        object.bytes[index] = (unsigned char)(0x40u + index);
    if (rewritten_atomic128_call(&object, expected, desired))
        return 3;
    for (index = 0; index < 16; ++index) {
        if (expected._0.bytes[index] != object.bytes[index])
            return 4;
        if (object.bytes[index] != (unsigned char)(0x40u + index))
            return 5;
    }

    write(1, pass, sizeof(pass) - 1);
    return 0;
}
