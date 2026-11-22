#include <stdint.h>

int __tcc_overflow(unsigned long long, unsigned long long, volatile void *,
                   int, int, int, int, int);

static int check(int condition, int code) {
    return condition ? 0 : code;
}

int main(void) {
    signed char s8 = 0;
    unsigned char u8 = 0;
    short s16 = 0;
    unsigned int u32 = 0;
    int s32 = 0;
    long long s64 = 0;
    unsigned long long u64 = 0;
    int error;

    error = check(__tcc_overflow(127, 1, &s8, 0, 0, 0, 1, 0) && s8 == -128, 1);
    if (error) return error;
    error = check(__tcc_overflow((unsigned long long)-128, 1, &s16,
                                 1, 0, 0, 2, 0) && s16 == 32767, 2);
    if (error) return error;
    error = check(__tcc_overflow(250, 10, &u8, 0, 1, 1, 1, 1) && u8 == 4, 3);
    if (error) return error;
    error = check(__tcc_overflow(1, 2, &u32, 1, 1, 1, 4, 1)
                  && u32 == 0xffffffffU, 4);
    if (error) return error;
    error = check(__tcc_overflow(50000, 50000, &s32, 2, 0, 0, 4, 0)
                  && (unsigned int)s32 == 0x9502f900U, 5);
    if (error) return error;
    error = check(__tcc_overflow((unsigned long long)-4, 5, &s64,
                                 2, 0, 0, 8, 0) == 0 && s64 == -20, 6);
    if (error) return error;
    error = check(__tcc_overflow(0xffffffffffffffffULL, 2, &u64,
                                 2, 1, 1, 8, 1)
                  && u64 == 0xfffffffffffffffeULL, 7);
    if (error) return error;
    error = check(__tcc_overflow((unsigned long long)0x8000000000000000ULL,
                                 (unsigned long long)-1, &s64,
                                 2, 1, 0, 8, 0)
                  == 0 && s64 == (long long)0x8000000000000000ULL, 8);
    if (error) return error;

    return 0;
}
