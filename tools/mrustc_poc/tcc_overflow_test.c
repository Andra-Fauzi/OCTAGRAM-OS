extern long write(int, const void *, unsigned long);

int main(void) {
    signed char s8 = 0;
    unsigned char u8 = 0;
    short s16 = 0;
    unsigned int u32 = 0;
    int s32 = 0;
    long long s64 = 0;
    unsigned long long u64 = 0;
    static const char pass[] = "TCC-OVERFLOW-PASS\n";

    if (!__builtin_add_overflow((signed char)127, (signed char)1, &s8)
        || s8 != -128)
        return 1;
    if (!__builtin_sub_overflow((short)-32768, (short)1, &s16)
        || s16 != 32767)
        return 2;
    if (!__builtin_add_overflow((unsigned char)250,
                                (unsigned char)10, &u8) || u8 != 4)
        return 3;
    if (!__builtin_sub_overflow(1U, 2U, &u32) || u32 != 0xffffffffU)
        return 4;
    if (!__builtin_mul_overflow(50000, 50000, &s32)
        || (unsigned int)s32 != 0x9502f900U)
        return 5;
    if (__builtin_mul_overflow(-4LL, 5LL, &s64) || s64 != -20)
        return 6;
    if (!__builtin_mul_overflow(0xffffffffffffffffULL, 2ULL, &u64)
        || u64 != 0xfffffffffffffffeULL)
        return 7;
    if (__builtin_mul_overflow(0x8000000000000000ULL, -1LL, &s64)
        || s64 != (long long)0x8000000000000000ULL)
        return 8;

    if (!__builtin_add_overflow(0x7fffffffffffffffLL, 1LL, &s64)
        || s64 != (long long)0x8000000000000000ULL)
        return 9;
    if (!__builtin_sub_overflow((long long)0x8000000000000000ULL, 1LL, &s64)
        || s64 != 0x7fffffffffffffffLL)
        return 10;
    if (!__builtin_sub_overflow(0ULL, 1ULL, &u64) || u64 != ~0ULL)
        return 11;
    if (!__builtin_add_overflow(~0ULL, 1ULL, &u64) || u64 != 0)
        return 12;

    write(1, pass, sizeof(pass) - 1);
    return 0;
}
